use arbor_core::{editor, util, EffectKind, NameTable, ReqKind};
use criterion::{criterion_group, criterion_main, Criterion};

/// Benchmark node parsing worst case, many substitutions and improperly sized buffer
fn stress_parse_node(c: &mut Criterion) {
    let mut name_table = NameTable::default();
    name_table.insert("Elle".to_string(), "Amberson".to_string());
    name_table.insert("Patrick".to_string(), "Breakforest".to_string());
    name_table.insert("Anna".to_string(), "Catmire".to_string());
    name_table.insert("Laura".to_string(), "Dagson".to_string());
    name_table.insert("John".to_string(), "Elliot".to_string());

    let text = "::Elle::xzunz::Anna::lxn ::Elle::cn::Patrick::o::Laura::sokxt::Patrick::eowln
    ::Patrick::::John::c::Patrick::iw qyyhr.jxhccpyvchze::Anna::ox hi::Laura::nlv::John::peh
    swvnismjs::John::p::Laura::::John::slu.hlqzkei jhrskiswe::John::::John::m.rx::Patrick::pk
    te::Elle::h::John::m,z,.jwtol::Elle::h rwvnpuqw::John::::John::::Elle::tnz::Elle::.kv.
    ::Laura::xyxml jrsei::John::jlsl nysn::Patrick::mvvu.up::Laura::jh,t,,jnwheu npnxqcowev
    ::Anna::,::Elle::.emiv::John::ezoqy::Elle::cppyxtos,miqphi::Elle::.q c::Patrick::nzms
    skno::Laura:: mkzn.::Patrick::x::John::s jhl::John::ow::John::nj hsk::Elle::ihwpens rx
    ::Patrick::nn..iurtxcou::Laura::hypkqoyqyz.iihu::Elle::umcpvl::Patrick::::Anna::.cjh,cn
    phey::Patrick::hxorixcyr::Anna::u::Anna::  heuneszejtwrkewiymmq::John::ynjvh::Laura::lvvtunm
    ::Laura::i.rk::Patrick::hk::Elle::knvmml::John::j::Anna::::Anna::pslllnmtcyjzesls moj ttm
    ::Elle::jrr,mh,::Anna:: pyl::Anna::owunpjve::John::::Laura:: ::Anna::xci::Patrick::p::Laura::
    l.iwn::Elle::lnjx::Laura::oyo::Anna::eq,n::Elle::ej.::Laura::hh";

    // bench part
    c.bench_function("stress_parse_node", |b| {
        b.iter(|| {
            let mut name_buf = String::with_capacity(1);
            let mut buf = String::with_capacity(1);
            util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
        })
    });
}

/// Benchmark standard node parsing case, few substitutions and pre-allocated buffer
fn quick_parse_node(c: &mut Criterion) {
    let mut name_table = NameTable::default();
    name_table.insert("vamp".to_string(), "Dracula".to_string());
    name_table.insert("king".to_string(), "King Laugh".to_string());

    let text = "::vamp::It is a strange world, a sad world, a world full of miseries, and woes, and 
    troubles. And yet when ::king:: come, he make them all dance to the tune he play. Bleeding hearts, 
    and dry bones of the churchyard, and tears that burn as they fall, all dance together to the music
    that he make with that smileless mouth of him. Ah, we men and women are like ropes drawn tight with
    strain that pull us different ways. Then tears come, and like the rain on the ropes, they brace us 
    up, until perhaps the strain become too great, and we break. But ::king:: he come like the
    sunshine, and he ease off the strain again, and we bear to go on with our labor, what it may be.";

    let mut name_buf = String::with_capacity(20);
    let mut buf = String::with_capacity(text.len() + 50);

    // bench part
    c.bench_function("quick_parse_node", |b| {
        b.iter(|| {
            util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
        })
    });
}

/// Test of a worst case type of scenario for undo/redo, where there is an extremely large number
/// of outgoing edges from a single node.
fn stress_undo_redo(c: &mut Criterion) {
    let mut editor = editor::Editor::new("undo_redo_bench", None).unwrap();
    let test_key = "cat";
    let test_name = "Behemoth";

    editor.new_name(test_key, test_name).unwrap();

    for i in 0..10000 {
        editor
            .new_node(test_key, &format!("test dialogue {}", i))
            .unwrap();
        editor
            .new_edge(
                &format!("test choice {}", i),
                0,
                i,
                ReqKind::No,
                EffectKind::No,
            )
            .unwrap();
    }
    // bench part
    c.bench_function("stress_undo_redo", |b| {
        b.iter(|| {
            for _ in 0..10 {
                editor.undo().unwrap();
            }
            for _ in 0..10 {
                editor.redo().unwrap();
            }
        })
    });
}

criterion_group!(
    benches,
    quick_parse_node,
    stress_parse_node,
    stress_undo_redo
);
criterion_main!(benches);
