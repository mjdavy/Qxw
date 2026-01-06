use anyhow::{Context, Result};
use qxw::format::{load_qxw, save_qxw2};
use qxw::model::{Puzzle, MXSZ};

use gtk4 as gtk;
use gtk::gio;
use gtk::gdk;
use gtk::prelude::*;

use glib::clone;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
struct UiHandles {
    drawing: gtk::DrawingArea,
    list: gtk::ListBox,
    poss_label: gtk::Label,
    entries_store: Rc<RefCell<Vec<EntryRow>>>,
}

#[derive(Debug, Clone)]
struct UiState {
    zoom_px: i32,
    curx: i32,
    cury: i32,
    dir: usize, // 0=Across, 1=Down (for gtype=0)
    filename: Option<PathBuf>,
    unsaved: bool,
}

impl UiState {
    fn new() -> Self {
        Self {
            zoom_px: 36,
            curx: 0,
            cury: 0,
            dir: 0,
            filename: None,
            unsaved: false,
        }
    }
}

#[derive(Debug, Clone)]
struct EntryRow {
    dir: usize,
    start: (i32, i32),
    number: i32,
    word: String,
}

#[derive(Debug, Clone)]
struct Snapshot {
    puzzle: Puzzle,
    state: UiState,
}

#[derive(Debug)]
struct UndoStack {
    items: Vec<Snapshot>,
    cursor: usize,
}

impl UndoStack {
    fn new(initial: Snapshot) -> Self {
        Self {
            items: vec![initial],
            cursor: 0,
        }
    }

    fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    fn can_redo(&self) -> bool {
        self.cursor + 1 < self.items.len()
    }

    fn push(&mut self, snap: Snapshot) {
        // drop redo tail
        self.items.truncate(self.cursor + 1);
        self.items.push(snap);
        self.cursor = self.items.len() - 1;

        // keep bounded
        const MAX: usize = 200;
        if self.items.len() > MAX {
            let drop_n = self.items.len() - MAX;
            self.items.drain(0..drop_n);
            self.cursor = self.cursor.saturating_sub(drop_n);
        }
    }

    fn undo(&mut self) -> Option<Snapshot> {
        if !self.can_undo() {
            return None;
        }
        self.cursor -= 1;
        Some(self.items[self.cursor].clone())
    }

    fn redo(&mut self) -> Option<Snapshot> {
        if !self.can_redo() {
            return None;
        }
        self.cursor += 1;
        Some(self.items[self.cursor].clone())
    }
}

fn main() -> Result<()> {
    // Start with an empty puzzle; files are loaded via the application's open signal.
    let mut puzzle = Puzzle::new();
    puzzle.gtype = 0;
    puzzle.width = 12;
    puzzle.height = 12;
    puzzle.title = "Untitled".to_string();
    puzzle.author = "".to_string();
    puzzle.compute_numbers();

    let pz = Rc::new(RefCell::new(puzzle));
    let state = Rc::new(RefCell::new({
        UiState::new()
    }));

    let undo = Rc::new(RefCell::new({
        let snap = Snapshot {
            puzzle: pz.borrow().clone(),
            state: state.borrow().clone(),
        };
        UndoStack::new(snap)
    }));

    let app = gtk::Application::builder()
        .application_id("com.example.qxw")
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    let handles: Rc<RefCell<Option<UiHandles>>> = Rc::new(RefCell::new(None));

    let pz2 = Rc::clone(&pz);
    let st2 = Rc::clone(&state);
    let ud2 = Rc::clone(&undo);
    let handles2 = Rc::clone(&handles);
    app.connect_activate(move |app| {
        if handles2.borrow().is_some() {
            return;
        }
        let ui = build_ui(app, Rc::clone(&pz2), Rc::clone(&st2), Rc::clone(&ud2));
        *handles2.borrow_mut() = Some(ui);
    });

    let pz3 = Rc::clone(&pz);
    let st3 = Rc::clone(&state);
    let ud3 = Rc::clone(&undo);
    let handles3 = Rc::clone(&handles);
    app.connect_open(move |app, files, _hint| {
        if let Some(f) = files.first() {
            if let Some(path) = f.path() {
                match load_qxw(&path).with_context(|| format!("loading {}", path.display())) {
                    Ok(mut loaded) => {
                        loaded.compute_numbers();
                        {
                            let mut stv = st3.borrow_mut();
                            stv.filename = Some(path);
                            stv.unsaved = false;
                            stv.curx = 0;
                            stv.cury = 0;
                            stv.dir = 0;
                        }
                        *pz3.borrow_mut() = loaded;
                        *ud3.borrow_mut() = UndoStack::new(Snapshot {
                            puzzle: pz3.borrow().clone(),
                            state: st3.borrow().clone(),
                        });

                        // If the UI is already built, refresh it.
                        if let Some(ui) = handles3.borrow().clone() {
                            resize_drawing(&ui.drawing, &pz3.borrow(), &st3.borrow());
                            rebuild_entry_list(&ui.list, &ui.entries_store, &pz3.borrow(), &st3.borrow());
                            update_poss_label(&ui.poss_label, &pz3.borrow(), &st3.borrow());
                            ui.drawing.queue_draw();
                        }
                    }
                    Err(e) => {
                        eprintln!("open failed: {e:#}");
                    }
                }
            }
        }
        app.activate();
    });

    // gtk-rs expects us to hand over control here.
    app.run();
    Ok(())
}

fn build_ui(
    app: &gtk::Application,
    pz: Rc<RefCell<Puzzle>>,
    state: Rc<RefCell<UiState>>,
    undo: Rc<RefCell<UndoStack>>,
) -> UiHandles {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Qxw (Rust UI)")
        .default_width(900)
        .default_height(650)
        .build();

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // Keep the default titlebar; custom HeaderBar has triggered GTK warnings on macOS.

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_position(560);

    // Left: grid scroller + drawing area.
    let grid_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();

    let drawing = gtk::DrawingArea::new();
    drawing.set_focusable(true);

    // Right: entry list + placeholder details.
    let right_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();

    let right_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    right_box.set_margin_top(10);
    right_box.set_margin_bottom(10);
    right_box.set_margin_start(10);
    right_box.set_margin_end(10);

    let poss_label = gtk::Label::new(Some(""));
    poss_label.set_xalign(0.0);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);

    let entries_store: Rc<RefCell<Vec<EntryRow>>> = Rc::new(RefCell::new(Vec::new()));

    let detail = gtk::TextView::new();
    detail.set_editable(false);
    detail.set_cursor_visible(false);
    detail.set_wrap_mode(gtk::WrapMode::Word);
    detail.set_vexpand(true);

    right_box.append(&poss_label);
    right_box.append(&list);
    right_box.append(&detail);
    right_scroller.set_child(Some(&right_box));

    grid_scroller.set_child(Some(&drawing));
    paned.set_start_child(Some(&grid_scroller));
    paned.set_end_child(Some(&right_scroller));

    vbox.append(&paned);
    window.set_child(Some(&vbox));

    // Keep the drawing area sized to the puzzle.
    resize_drawing(&drawing, &pz.borrow(), &state.borrow());

    // Populate the entry list.
    rebuild_entry_list(&list, &entries_store, &pz.borrow(), &state.borrow());
    update_poss_label(&poss_label, &pz.borrow(), &state.borrow());

    // Drawing
    {
        let pz = Rc::clone(&pz);
        let st = Rc::clone(&state);
        drawing.set_draw_func(move |_da, cr, _w, _h| {
            let pz = pz.borrow();
            let st = st.borrow();
            draw_grid_gtype0(cr, &pz, &st);
        });
    }

    // Mouse click -> move cursor (and toggle bars when clicking near edges)
    {
        let pz = Rc::clone(&pz);
        let st = Rc::clone(&state);
        let drawing2 = drawing.clone();
        let list2 = list.clone();
        let poss2 = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let undo2 = Rc::clone(&undo);

        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_g, _n, x, y| {
            let mut st = st.borrow_mut();
            let mut pz = pz.borrow_mut();
            if pz.gtype != 0 {
                return;
            }
            let cell = st.zoom_px.max(4) as f64;
            let cx = (x / cell).floor() as i32;
            let cy = (y / cell).floor() as i32;
            if pz.is_ingrid(cx, cy) {
                st.curx = cx;
                st.cury = cy;

                // If click is near right/bottom edge, toggle a bar.
                let lx = x - (cx as f64) * cell;
                let ly = y - (cy as f64) * cell;
                let edge = cell * 0.20;

                let mut changed = false;
                if lx > cell - edge {
                    changed |= toggle_bar(&mut pz, cx, cy, 0);
                } else if ly > cell - edge {
                    changed |= toggle_bar(&mut pz, cx, cy, 1);
                }
                if changed {
                    st.unsaved = true;
                    pz.compute_numbers();
                    push_undo(&mut undo2.borrow_mut(), &pz, &st);
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                }

                update_poss_label(&poss2, &pz, &st);
                drawing2.queue_draw();
            }
            drawing2.grab_focus();
        });
        drawing.add_controller(click);
    }

    // Keyboard editing / navigation (subset of original key bindings)
    {
        let pz = Rc::clone(&pz);
        let st = Rc::clone(&state);
        let drawing2 = drawing.clone();
        let list2 = list.clone();
        let poss2 = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let undo2 = Rc::clone(&undo);
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_k, keyval, _keycode, state_mods| {
            let mut pz = pz.borrow_mut();
            let mut st = st.borrow_mut();

            let ctrl = state_mods.contains(gdk::ModifierType::CONTROL_MASK);

            if ctrl && keyval == gdk::Key::z {
                if let Some(snap) = undo2.borrow_mut().undo() {
                    *pz = snap.puzzle;
                    *st = snap.state;
                    resize_drawing(&drawing2, &pz, &st);
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                    update_poss_label(&poss2, &pz, &st);
                    drawing2.queue_draw();
                }
                return gdk::glib::Propagation::Stop;
            }

            if ctrl && keyval == gdk::Key::y {
                if let Some(snap) = undo2.borrow_mut().redo() {
                    *pz = snap.puzzle;
                    *st = snap.state;
                    resize_drawing(&drawing2, &pz, &st);
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                    update_poss_label(&poss2, &pz, &st);
                    drawing2.queue_draw();
                }
                return gdk::glib::Propagation::Stop;
            }

            // Ctrl+S saves back to the last loaded filename (no dialog yet)
            if ctrl && keyval == gdk::Key::s {
                if let Some(path) = &st.filename {
                    if save_qxw2(&pz, path).is_ok() {
                        st.unsaved = false;
                    }
                }
                return gdk::glib::Propagation::Stop;
            }

            if pz.gtype != 0 {
                return gdk::glib::Propagation::Proceed;
            }

            // Letters/digits or Tab => set char, then advance.
            if !ctrl {
                if keyval == gdk::Key::Tab {
                    if set_cell_char(&mut pz, st.curx, st.cury, b' ') {
                        st.unsaved = true;
                        pz.compute_numbers();
                        push_undo(&mut undo2.borrow_mut(), &pz, &st);
                    }
                    let dir = st.dir;
                    let mut nx = st.curx;
                    let mut ny = st.cury;
                    step_forw_if_ingrid(&pz, &mut nx, &mut ny, dir);
                    st.curx = nx;
                    st.cury = ny;
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                    update_poss_label(&poss2, &pz, &st);
                    drawing2.queue_draw();
                    return gdk::glib::Propagation::Stop;
                }

                if let Some(ch) = keyval_to_ascii_upper(keyval) {
                    if set_cell_char(&mut pz, st.curx, st.cury, ch) {
                        st.unsaved = true;
                        pz.compute_numbers();
                        push_undo(&mut undo2.borrow_mut(), &pz, &st);
                    }
                    let dir = st.dir;
                    let mut nx = st.curx;
                    let mut ny = st.cury;
                    step_forw_if_ingrid(&pz, &mut nx, &mut ny, dir);
                    st.curx = nx;
                    st.cury = ny;
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                    update_poss_label(&poss2, &pz, &st);
                    drawing2.queue_draw();
                    return gdk::glib::Propagation::Stop;
                }
            }

            // Toggle block at cursor
            if !ctrl && keyval == gdk::Key::x {
                if toggle_block(&mut pz, st.curx, st.cury) {
                    st.unsaved = true;
                    pz.compute_numbers();
                    push_undo(&mut undo2.borrow_mut(), &pz, &st);
                    resize_drawing(&drawing2, &pz, &st);
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                    update_poss_label(&poss2, &pz, &st);
                    drawing2.queue_draw();
                }
                return gdk::glib::Propagation::Stop;
            }

            // Toggle bar in current direction
            if !ctrl && keyval == gdk::Key::bar {
                if toggle_bar(&mut pz, st.curx, st.cury, st.dir) {
                    st.unsaved = true;
                    pz.compute_numbers();
                    push_undo(&mut undo2.borrow_mut(), &pz, &st);
                    rebuild_entry_list(&list2, &entries_store2, &pz, &st);
                    update_poss_label(&poss2, &pz, &st);
                    drawing2.queue_draw();
                }
                return gdk::glib::Propagation::Stop;
            }

            match keyval {
                gdk::Key::Left => {
                    let nx = st.curx - 1;
                    if pz.is_ingrid(nx, st.cury) {
                        st.curx = nx;
                    }
                }
                gdk::Key::Right => {
                    let nx = st.curx + 1;
                    if pz.is_ingrid(nx, st.cury) {
                        st.curx = nx;
                    }
                }
                gdk::Key::Up => {
                    let ny = st.cury - 1;
                    if pz.is_ingrid(st.curx, ny) {
                        st.cury = ny;
                    }
                }
                gdk::Key::Down => {
                    let ny = st.cury + 1;
                    if pz.is_ingrid(st.curx, ny) {
                        st.cury = ny;
                    }
                }
                gdk::Key::Page_Up => {
                    st.dir = (st.dir + 1) % 2;
                }
                gdk::Key::Page_Down => {
                    st.dir = (st.dir + 1) % 2;
                }
                gdk::Key::space => {
                    let dir = st.dir;
                    let mut nx = st.curx;
                    let mut ny = st.cury;
                    step_forw_if_ingrid(&pz, &mut nx, &mut ny, dir);
                    st.curx = nx;
                    st.cury = ny;
                }
                gdk::Key::BackSpace => {
                    let dir = st.dir;
                    let mut nx = st.curx;
                    let mut ny = st.cury;
                    step_back_if_ingrid(&pz, &mut nx, &mut ny, dir);
                    st.curx = nx;
                    st.cury = ny;
                }
                _ => return gdk::glib::Propagation::Proceed,
            }

            update_poss_label(&poss2, &pz, &st);
            drawing2.queue_draw();
            gdk::glib::Propagation::Stop
        });
        drawing.add_controller(key);
    }

    // List row activated -> jump cursor
    {
        let pz = Rc::clone(&pz);
        let st = Rc::clone(&state);
        let drawing2 = drawing.clone();
        let poss2 = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        list.connect_row_activated(move |_lb, row| {
            let idx = row.index();
            if idx < 0 {
                return;
            }
            let entries = entries_store2.borrow();
            let Some(data) = entries.get(idx as usize).cloned() else {
                return;
            };
            let mut st = st.borrow_mut();
            let pz = pz.borrow();
            if pz.gtype != 0 {
                return;
            }
            st.dir = data.dir;
            st.curx = data.start.0;
            st.cury = data.start.1;
            update_poss_label(&poss2, &pz, &st);
            drawing2.queue_draw();
        });
    }

    // Menu actions
    install_actions(
        app,
        &window,
        &drawing,
        &list,
        &entries_store,
        &poss_label,
        Rc::clone(&pz),
        Rc::clone(&state),
        Rc::clone(&undo),
    );

    window.show();

    UiHandles {
        drawing,
        list,
        poss_label,
        entries_store,
    }
}

fn keyval_to_ascii_upper(keyval: gdk::Key) -> Option<u8> {
    // Accept A-Z and 0-9.
    if let Some(ch) = keyval.to_unicode() {
        if ch.is_ascii_alphanumeric() {
            let up = ch.to_ascii_uppercase();
            if up.is_ascii_alphanumeric() {
                return Some(up as u8);
            }
        }
    }
    None
}

fn set_cell_char(pz: &mut Puzzle, x: i32, y: i32, ch: u8) -> bool {
    if !pz.is_ingrid(x, y) {
        return false;
    }
    let sq = pz.square_mut(x, y).unwrap();
    // Don’t write into blocks/cutouts.
    if (sq.fl & 0x09) != 0 {
        return false;
    }
    if sq.ch == ch {
        return false;
    }
    sq.ch = ch;
    true
}

fn toggle_block(pz: &mut Puzzle, x: i32, y: i32) -> bool {
    if !pz.is_ingrid(x, y) {
        return false;
    }
    let sq = pz.square_mut(x, y).unwrap();
    if (sq.fl & 0x08) != 0 {
        return false;
    }
    let was_block = (sq.fl & 0x01) != 0;
    if was_block {
        sq.fl &= !0x01;
    } else {
        sq.fl |= 0x01;
        sq.ch = b' ';
    }
    true
}

fn toggle_bar(pz: &mut Puzzle, x: i32, y: i32, d: usize) -> bool {
    if pz.gtype != 0 {
        return false;
    }
    if d > 1 {
        return false;
    }
    if !pz.is_ingrid(x, y) {
        return false;
    }
    let (nx, ny) = if d == 0 { (x + 1, y) } else { (x, y + 1) };
    if !pz.is_ingrid(nx, ny) {
        return false;
    }
    let sq = pz.square_mut(x, y).unwrap();
    let bit = 1u32 << (d as u32);
    sq.bars ^= bit;
    true
}

fn resize_drawing(drawing: &gtk::DrawingArea, pz: &Puzzle, state: &UiState) {
    let w = pz.width.max(1);
    let h = pz.height.max(1);
    drawing.set_content_width((w * state.zoom_px) as i32 + 2);
    drawing.set_content_height((h * state.zoom_px) as i32 + 2);
}

fn current_light_start(pz: &Puzzle, x: i32, y: i32, d: usize) -> Option<(i32, i32)> {
    if !pz.is_clear(x, y) {
        return None;
    }
    let mut sx = x;
    let mut sy = y;
    let x0 = x;
    let y0 = y;
    while pz.clear_before(sx, sy, d) {
        pz.step_back(&mut sx, &mut sy, d);
        if sx == x0 && sy == y0 {
            return None;
        }
    }
    Some((sx, sy))
}

fn update_poss_label(label: &gtk::Label, pz: &Puzzle, st: &UiState) {
    if pz.gtype != 0 {
        label.set_text("");
        return;
    }
    let Some((sx, sy)) = current_light_start(pz, st.curx, st.cury, st.dir) else {
        label.set_text("");
        return;
    };
    let number = pz.square(sx, sy).map(|q| q.number).unwrap_or(-1);
    let word = pz.get_word(sx, sy, st.dir).unwrap_or_default();
    let dir_name = if st.dir == 0 { "Across" } else { "Down" };
    label.set_text(&format!("{} {}: {}", dir_name, number, word.replace(' ', ".")));
}

fn rebuild_entry_list(
    list: &gtk::ListBox,
    entries_store: &Rc<RefCell<Vec<EntryRow>>>,
    pz: &Puzzle,
    st: &UiState,
) {
    // Clear children
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    entries_store.borrow_mut().clear();

    if pz.gtype != 0 {
        return;
    }

    // collect in reading order, grouped by direction
    let mut entries: Vec<EntryRow> = Vec::new();
    for (dir, x, y, word, number) in pz.iter_lights() {
        entries.push(EntryRow {
            dir,
            start: (x, y),
            number,
            word,
        });
    }

    *entries_store.borrow_mut() = entries.clone();

    // compute current start so we can select
    let current = current_light_start(pz, st.curx, st.cury, st.dir);

    let mut selected_row: Option<gtk::ListBoxRow> = None;

    for e in entries {
        let text = format!("{} {}  {}", if e.dir == 0 { "A" } else { "D" }, e.number, e.word.replace(' ', "."));
        let row = gtk::ListBoxRow::new();
        let lbl = gtk::Label::new(Some(&text));
        lbl.set_xalign(0.0);
        lbl.set_margin_top(2);
        lbl.set_margin_bottom(2);
        lbl.set_margin_start(4);
        lbl.set_margin_end(4);
        row.set_child(Some(&lbl));
        list.append(&row);

        if Some(e.start) == current && e.dir == st.dir {
            selected_row = Some(row);
        }
    }

    if let Some(r) = selected_row {
        list.select_row(Some(&r));
    }
}

fn push_undo(undo: &mut UndoStack, pz: &Puzzle, st: &UiState) {
    undo.push(Snapshot {
        puzzle: pz.clone(),
        state: st.clone(),
    });
}

fn install_actions(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    drawing: &gtk::DrawingArea,
    list: &gtk::ListBox,
    entries_store: &Rc<RefCell<Vec<EntryRow>>>,
    poss_label: &gtk::Label,
    pz: Rc<RefCell<Puzzle>>,
    state: Rc<RefCell<UiState>>,
    undo: Rc<RefCell<UndoStack>>,
) {
    // Clone GObjects so closures can be 'static.
    let app = app.clone();
    let window = window.clone();
    let drawing = drawing.clone();
    let list = list.clone();
    let poss_label = poss_label.clone();
    let entries_store = Rc::clone(entries_store);

    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    file.append(Some("New…"), Some("app.new"));
    file.append(Some("Open…"), Some("app.open"));
    file.append(Some("Save"), Some("app.save"));
    file.append(Some("Save As…"), Some("app.save_as"));
    file.append(Some("Quit"), Some("app.quit"));
    menu.append_submenu(Some("File"), &file);

    let edit = gio::Menu::new();
    edit.append(Some("Undo"), Some("app.undo"));
    edit.append(Some("Redo"), Some("app.redo"));
    edit.append(Some("Toggle Block"), Some("app.toggle_block"));
    edit.append(Some("Toggle Bar"), Some("app.toggle_bar"));
    menu.append_submenu(Some("Edit"), &edit);

    app.set_menubar(Some(&menu));

    // New
    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let undo = Rc::clone(&undo);
        let drawing = drawing.clone();
        let list = list.clone();
        let poss_label = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let window = window.clone();
        let action = gio::SimpleAction::new("new", None);
        action.connect_activate(move |_, _| {
            let dialog = gtk::Dialog::with_buttons(
                Some("New Puzzle"),
                Some(&window),
                gtk::DialogFlags::MODAL,
                &[
                    ("Cancel", gtk::ResponseType::Cancel),
                    ("Create", gtk::ResponseType::Accept),
                ],
            );
            dialog.set_default_response(gtk::ResponseType::Accept);

            let content = dialog.content_area();
            let grid = gtk::Grid::new();
            grid.set_row_spacing(8);
            grid.set_column_spacing(8);
            grid.set_margin_top(12);
            grid.set_margin_bottom(12);
            grid.set_margin_start(12);
            grid.set_margin_end(12);

            let width_label = gtk::Label::new(Some("Width:"));
            width_label.set_xalign(0.0);
            let height_label = gtk::Label::new(Some("Height:"));
            height_label.set_xalign(0.0);

            let max = MXSZ as f64;
            let width_adj = gtk::Adjustment::new(12.0, 1.0, max, 1.0, 5.0, 0.0);
            let height_adj = gtk::Adjustment::new(12.0, 1.0, max, 1.0, 5.0, 0.0);
            let width_spin = gtk::SpinButton::new(Some(&width_adj), 1.0, 0);
            let height_spin = gtk::SpinButton::new(Some(&height_adj), 1.0, 0);
            width_spin.set_numeric(true);
            height_spin.set_numeric(true);
            width_spin.set_hexpand(true);
            height_spin.set_hexpand(true);

            grid.attach(&width_label, 0, 0, 1, 1);
            grid.attach(&width_spin, 1, 0, 1, 1);
            grid.attach(&height_label, 0, 1, 1, 1);
            grid.attach(&height_spin, 1, 1, 1, 1);

            content.append(&grid);

            dialog.connect_response(
                clone!(@strong pz, @strong state, @strong undo, @strong drawing, @strong list, @strong poss_label, @strong entries_store2, @strong width_spin, @strong height_spin => move |dlg, resp| {
                    if resp == gtk::ResponseType::Accept {
                        let w = width_spin.value_as_int().max(1).min(MXSZ as i32);
                        let h = height_spin.value_as_int().max(1).min(MXSZ as i32);

                        let mut pzv = Puzzle::new();
                        pzv.gtype = 0;
                        pzv.width = w;
                        pzv.height = h;
                        pzv.title = "Untitled".to_string();
                        pzv.compute_numbers();

                        let mut stv = UiState::new();
                        stv.filename = None;
                        stv.unsaved = false;
                        stv.curx = 0;
                        stv.cury = 0;
                        stv.dir = 0;

                        *pz.borrow_mut() = pzv;
                        *state.borrow_mut() = stv;
                        *undo.borrow_mut() = UndoStack::new(Snapshot {
                            puzzle: pz.borrow().clone(),
                            state: state.borrow().clone(),
                        });

                        resize_drawing(&drawing, &pz.borrow(), &state.borrow());
                        rebuild_entry_list(&list, &entries_store2, &pz.borrow(), &state.borrow());
                        update_poss_label(&poss_label, &pz.borrow(), &state.borrow());
                        drawing.queue_draw();
                    }
                    dlg.close();
                }),
            );

            dialog.show();
        });
        app.add_action(&action);
    }

    // Quit
    {
        let action = gio::SimpleAction::new("quit", None);
        action.connect_activate(clone!(@weak app => move |_, _| {
            app.quit();
        }));
        app.add_action(&action);
    }

    // Undo/Redo
    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let undo = Rc::clone(&undo);
        let drawing = drawing.clone();
        let list = list.clone();
        let poss_label = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let action = gio::SimpleAction::new("undo", None);
        action.connect_activate(move |_, _| {
            if let Some(snap) = undo.borrow_mut().undo() {
                *pz.borrow_mut() = snap.puzzle;
                *state.borrow_mut() = snap.state;
                resize_drawing(&drawing, &pz.borrow(), &state.borrow());
                rebuild_entry_list(&list, &entries_store2, &pz.borrow(), &state.borrow());
                update_poss_label(&poss_label, &pz.borrow(), &state.borrow());
                drawing.queue_draw();
            }
        });
        app.add_action(&action);
    }
    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let undo = Rc::clone(&undo);
        let drawing = drawing.clone();
        let list = list.clone();
        let poss_label = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let action = gio::SimpleAction::new("redo", None);
        action.connect_activate(move |_, _| {
            if let Some(snap) = undo.borrow_mut().redo() {
                *pz.borrow_mut() = snap.puzzle;
                *state.borrow_mut() = snap.state;
                resize_drawing(&drawing, &pz.borrow(), &state.borrow());
                rebuild_entry_list(&list, &entries_store2, &pz.borrow(), &state.borrow());
                update_poss_label(&poss_label, &pz.borrow(), &state.borrow());
                drawing.queue_draw();
            }
        });
        app.add_action(&action);
    }

    // Toggle Block/Bar
    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let undo = Rc::clone(&undo);
        let drawing2 = drawing.clone();
        let list2 = list.clone();
        let poss2 = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let action = gio::SimpleAction::new("toggle_block", None);
        action.connect_activate(move |_, _| {
            let mut pzv = pz.borrow_mut();
            let mut stv = state.borrow_mut();
            if toggle_block(&mut pzv, stv.curx, stv.cury) {
                stv.unsaved = true;
                pzv.compute_numbers();
                push_undo(&mut undo.borrow_mut(), &pzv, &stv);
                resize_drawing(&drawing2, &pzv, &stv);
                rebuild_entry_list(&list2, &entries_store2, &pzv, &stv);
                update_poss_label(&poss2, &pzv, &stv);
                drawing2.queue_draw();
            }
        });
        app.add_action(&action);
    }
    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let undo = Rc::clone(&undo);
        let drawing2 = drawing.clone();
        let list2 = list.clone();
        let poss2 = poss_label.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let action = gio::SimpleAction::new("toggle_bar", None);
        action.connect_activate(move |_, _| {
            let mut pzv = pz.borrow_mut();
            let mut stv = state.borrow_mut();
            if toggle_bar(&mut pzv, stv.curx, stv.cury, stv.dir) {
                stv.unsaved = true;
                pzv.compute_numbers();
                push_undo(&mut undo.borrow_mut(), &pzv, &stv);
                rebuild_entry_list(&list2, &entries_store2, &pzv, &stv);
                update_poss_label(&poss2, &pzv, &stv);
                drawing2.queue_draw();
            }
        });
        app.add_action(&action);
    }

    // Open/Save/Save As with GTK4 FileChooserNative
    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let undo = Rc::clone(&undo);
        let drawing = drawing.clone();
        let list = list.clone();
        let poss_label = poss_label.clone();
        let window = window.clone();
        let entries_store2 = Rc::clone(&entries_store);
        let action = gio::SimpleAction::new("open", None);
        action.connect_activate(move |_, _| {
            let dialog = gtk::FileChooserNative::new(
                Some("Open"),
                Some(&window),
                gtk::FileChooserAction::Open,
                Some("Open"),
                Some("Cancel"),
            );
            dialog.connect_response(clone!(@strong pz, @strong state, @strong undo, @strong drawing, @strong list, @strong poss_label, @strong entries_store2 => move |dlg, resp| {
                if resp == gtk::ResponseType::Accept {
                    let Some(file) = dlg.file() else {
                        dlg.hide();
                        return;
                    };
                    let Some(path) = file.path() else {
                        dlg.hide();
                        return;
                    };
                    let mut loaded = match load_qxw(&path) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("open failed: {e:#}");
                            dlg.hide();
                            return;
                        }
                    };
                    loaded.compute_numbers();

                    let mut stv = state.borrow_mut();
                    stv.filename = Some(path);
                    stv.unsaved = false;
                    stv.curx = 0;
                    stv.cury = 0;
                    stv.dir = 0;

                    *pz.borrow_mut() = loaded;
                    *undo.borrow_mut() = UndoStack::new(Snapshot {
                        puzzle: pz.borrow().clone(),
                        state: state.borrow().clone(),
                    });

                    resize_drawing(&drawing, &pz.borrow(), &stv);
                    rebuild_entry_list(&list, &entries_store2, &pz.borrow(), &stv);
                    update_poss_label(&poss_label, &pz.borrow(), &stv);
                    drawing.queue_draw();
                }
                dlg.hide();
            }));
            dialog.show();
        });
        app.add_action(&action);
    }

    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let window = window.clone();
        let app2 = app.clone();
        let action = gio::SimpleAction::new("save", None);
        action.connect_activate(move |_, _| {
            let pzv = pz.borrow();
            let mut stv = state.borrow_mut();
            let Some(path) = &stv.filename else {
                // fall back to Save As
                drop(pzv);
                drop(stv);
                app2.activate_action("save_as", None);
                return;
            };
            if let Err(e) = save_qxw2(&pzv, path) {
                eprintln!("save failed: {e:#}");
                return;
            }
            stv.unsaved = false;
            let _ = &window;
        });
        app.add_action(&action);
    }

    {
        let pz = Rc::clone(&pz);
        let state = Rc::clone(&state);
        let window = window.clone();
        let action = gio::SimpleAction::new("save_as", None);
        action.connect_activate(move |_, _| {
            let dialog = gtk::FileChooserNative::new(
                Some("Save As"),
                Some(&window),
                gtk::FileChooserAction::Save,
                Some("Save"),
                Some("Cancel"),
            );
            dialog.connect_response(clone!(@strong pz, @strong state => move |dlg, resp| {
                if resp == gtk::ResponseType::Accept {
                    let Some(file) = dlg.file() else {
                        dlg.hide();
                        return;
                    };
                    let Some(path) = file.path() else {
                        dlg.hide();
                        return;
                    };
                    let pzv = pz.borrow();
                    if let Err(e) = save_qxw2(&pzv, &path) {
                        eprintln!("save-as failed: {e:#}");
                        dlg.hide();
                        return;
                    }
                    let mut stv = state.borrow_mut();
                    stv.filename = Some(path);
                    stv.unsaved = false;
                }
                dlg.hide();
            }));
            dialog.show();
        });
        app.add_action(&action);
    }

    // Give some common accelerators.
    app.set_accels_for_action("app.open", &["<Primary>o"]);
    app.set_accels_for_action("app.save", &["<Primary>s"]);
    app.set_accels_for_action("app.save_as", &["<Primary><Shift>s"]);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);
    app.set_accels_for_action("app.undo", &["<Primary>z"]);
    app.set_accels_for_action("app.redo", &["<Primary>y"]);

    // Menu actions call redraw/update directly.
}

fn step_forw_if_ingrid(pz: &Puzzle, x: &mut i32, y: &mut i32, d: usize) {
    let mut nx = *x;
    let mut ny = *y;
    pz.step_forw(&mut nx, &mut ny, d);
    if pz.is_ingrid(nx, ny) {
        *x = nx;
        *y = ny;
    }
}

fn step_back_if_ingrid(pz: &Puzzle, x: &mut i32, y: &mut i32, d: usize) {
    let mut nx = *x;
    let mut ny = *y;
    pz.step_back(&mut nx, &mut ny, d);
    if pz.is_ingrid(nx, ny) {
        *x = nx;
        *y = ny;
    }
}

fn draw_grid_gtype0(cr: &gtk::cairo::Context, pz: &Puzzle, st: &UiState) {
    let cell = st.zoom_px.max(4) as f64;
    let w = pz.width.max(1);
    let h = pz.height.max(1);

    // Background
    cr.set_source_rgb(0.92, 0.92, 0.92);
    cr.paint().ok();

    // Grid background
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(0.0, 0.0, (w as f64) * cell, (h as f64) * cell);
    cr.fill().ok();

    // Cells
    for y in 0..h {
        for x in 0..w {
            if !pz.is_ingrid(x, y) {
                continue;
            }
            let sq = pz.square(x, y).unwrap();
            let rx = (x as f64) * cell;
            let ry = (y as f64) * cell;

            if (sq.fl & 0x08) != 0 {
                // cutout
                cr.set_source_rgb(0.92, 0.92, 0.92);
                cr.rectangle(rx, ry, cell, cell);
                cr.fill().ok();
                continue;
            }

            if (sq.fl & 0x01) != 0 {
                // blocked
                cr.set_source_rgb(0.0, 0.0, 0.0);
                cr.rectangle(rx, ry, cell, cell);
                cr.fill().ok();
                continue;
            }

            // Cursor highlight
            if x == st.curx && y == st.cury {
                cr.set_source_rgb(1.0, 1.0, 0.75);
                cr.rectangle(rx, ry, cell, cell);
                cr.fill().ok();
            }

            // Number
            if sq.number >= 0 {
                cr.set_source_rgb(0.0, 0.0, 0.0);
                cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
                cr.set_font_size((cell * 0.22).max(8.0));
                cr.move_to(rx + cell * 0.08, ry + cell * 0.28);
                let _ = cr.show_text(&format!("{}", sq.number));
            }

            // Letter
            if sq.ch != b' ' {
                cr.set_source_rgb(0.0, 0.0, 0.0);
                cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
                cr.set_font_size((cell * 0.52).max(12.0));

                let ch = (sq.ch as char).to_string();
                let te = cr.text_extents(&ch).unwrap();
                let tx = rx + (cell - te.width()) / 2.0 - te.x_bearing();
                // Center vertically using font ascent/descent rather than text extents;
                // this keeps the baseline from drifting low.
                let fe = cr.font_extents().unwrap();
                let ty = ry + (cell + fe.ascent() - fe.descent()) / 2.0;
                cr.move_to(tx, ty);
                let _ = cr.show_text(&ch);
            }
        }
    }

    // Grid lines
    cr.set_source_rgb(0.0, 0.0, 0.0);
    cr.set_line_width(1.0);
    for i in 0..=w {
        let x = (i as f64) * cell;
        cr.move_to(x, 0.0);
        cr.line_to(x, (h as f64) * cell);
    }
    for j in 0..=h {
        let y = (j as f64) * cell;
        cr.move_to(0.0, y);
        cr.line_to((w as f64) * cell, y);
    }
    cr.stroke().ok();

    // Bars (thicker)
    cr.set_source_rgb(0.0, 0.0, 0.0);
    cr.set_line_width((cell * 0.12).max(3.0));
    for y in 0..h {
        for x in 0..w {
            if !pz.is_ingrid(x, y) {
                continue;
            }
            if pz.is_bar(x, y, 0) {
                // vertical bar between (x,y) and (x+1,y)
                let bx = ((x + 1) as f64) * cell;
                let by0 = (y as f64) * cell;
                let by1 = ((y + 1) as f64) * cell;
                cr.move_to(bx, by0);
                cr.line_to(bx, by1);
            }
            if pz.is_bar(x, y, 1) {
                // horizontal bar between (x,y) and (x,y+1)
                let by = ((y + 1) as f64) * cell;
                let bx0 = (x as f64) * cell;
                let bx1 = ((x + 1) as f64) * cell;
                cr.move_to(bx0, by);
                cr.line_to(bx1, by);
            }
        }
    }
    cr.stroke().ok();
}
