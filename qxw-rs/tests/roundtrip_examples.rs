use qxw::format::{load_qxw, save_qxw2};
use tempfile::tempdir;

fn assert_square_eq(a: &qxw::model::Square, b: &qxw::model::Square) {
    assert_eq!(a.bars, b.bars);
    assert_eq!(a.merge, b.merge);
    assert_eq!(a.fl, b.fl);
    assert_eq!(a.dsel, b.dsel);
    assert_eq!(a.ch, b.ch);
    assert_eq!(a.sp.bgcol, b.sp.bgcol);
    assert_eq!(a.sp.fgcol, b.sp.fgcol);
    assert_eq!(a.sp.ten, b.sp.ten);
    assert_eq!(a.sp.spor, b.sp.spor);
    for i in 0..qxw::model::MAXNDIR {
        assert_eq!(a.lp[i].dmask, b.lp[i].dmask);
        assert_eq!(a.lp[i].emask, b.lp[i].emask);
        assert_eq!(a.lp[i].ten, b.lp[i].ten);
        assert_eq!(a.lp[i].lpor, b.lp[i].lpor);
    }
}

#[test]
fn roundtrip_bar_example() {
    roundtrip("../examples/bar.qxw");
}

#[test]
fn roundtrip_circle_example() {
    roundtrip("../examples/circle.qxw");
}

fn roundtrip(path: &str) {
    let mut p1 = load_qxw(path).expect("load");
    p1.compute_numbers();

    let dir = tempdir().unwrap();
    let out = dir.path().join("out.qxw");
    save_qxw2(&p1, &out).expect("save");

    let mut p2 = load_qxw(&out).expect("reload");
    p2.compute_numbers();

    assert_eq!(p1.gtype, p2.gtype);
    assert_eq!(p1.width, p2.width);
    assert_eq!(p1.height, p2.height);
    assert_eq!(p1.symmr, p2.symmr);
    assert_eq!(p1.symmm, p2.symmm);
    assert_eq!(p1.symmd, p2.symmd);
    assert_eq!(p1.title, p2.title);
    assert_eq!(p1.author, p2.author);

    assert_eq!(p1.dlp.dmask, p2.dlp.dmask);
    assert_eq!(p1.dlp.emask, p2.dlp.emask);
    assert_eq!(p1.dlp.ten, p2.dlp.ten);

    assert_eq!(p1.dsp.bgcol, p2.dsp.bgcol);
    assert_eq!(p1.dsp.fgcol, p2.dsp.fgcol);
    assert_eq!(p1.dsp.ten, p2.dsp.ten);

    assert_eq!(p1.treatmode, p2.treatmode);
    assert_eq!(p1.tambaw, p2.tambaw);
    assert_eq!(p1.tpifname, p2.tpifname);
    assert_eq!(p1.treatmsg, p2.treatmsg);
    assert_eq!(p1.dfnames, p2.dfnames);
    assert_eq!(p1.dsfilters, p2.dsfilters);
    assert_eq!(p1.dafilters, p2.dafilters);

    for y in 0..p1.height {
        for x in 0..p1.width {
            let s1 = p1.square(x, y).unwrap();
            let s2 = p2.square(x, y).unwrap();
            assert_square_eq(s1, s2);
            assert_eq!(s1.number, s2.number);
        }
    }
}
