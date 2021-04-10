use arbor_core::*;
use criterion::{criterion_group, criterion_main, Criterion};

/// Benchmark node parsing worst case, many substitutions and improperly sized buffer
fn stress_parse_node(c: &mut Criterion) {
    let mut name_table = NameTable::default();
    name_table.insert(
        KeyString::from("Elle").unwrap(),
        NameString::from("Amberson").unwrap(),
    );
    name_table.insert(
        KeyString::from("Patrick").unwrap(),
        NameString::from("Breakforest").unwrap(),
    );
    name_table.insert(
        KeyString::from("Anna").unwrap(),
        NameString::from("Catmire").unwrap(),
    );
    name_table.insert(
        KeyString::from("Laura").unwrap(),
        NameString::from("Dagson").unwrap(),
    );
    name_table.insert(
        KeyString::from("John").unwrap(),
        NameString::from("Elliot").unwrap(),
    );

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
            cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
        })
    });
}

/// Benchmark standard node parsing case, few substitutions and pre-allocated buffer
fn quick_parse_node(c: &mut Criterion) {
    let mut name_table = NameTable::default();
    name_table.insert(
        KeyString::from("vamp").unwrap(),
        NameString::from("Dracula").unwrap(),
    );
    name_table.insert(
        KeyString::from("king").unwrap(),
        NameString::from("King Laugh").unwrap(),
    );

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
            cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
        })
    });
}

/// Investigate best path forward for undo/redo by comparing state update methods
fn copy_diff(c: &mut Criterion) {
    let mut state = EditorState::new(DialogueTreeData::default());
    cmd::new::Project::new("copy_vs_diff".to_string(), true)
        .execute(&mut state)
        .unwrap();
    cmd::new::Name::new(
        KeyString::from("cat").unwrap(),
        NameString::from("Behemoth").unwrap(),
    )
    .execute(&mut state)
    .unwrap();
    assert_eq!(state.active.name_table.get("cat").unwrap(), "Behemoth");

    // add a bunch of nodes
    for i in 0..10000 {
        cmd::new::Node::new("cat".to_string(), format!("{}", i))
            .execute(&mut state)
            .unwrap();
    }

    // placeholder variables for redo
    let mut future_list = Vec::<usize>::with_capacity(1);
    let mut future_stack = Vec::<u8>::with_capacity(1000);

    c.bench_function("copy_diff", |b| {
        b.iter(|| {
            // get matchign baseline of backup and active copy by cloning
            state.backup = state.active.clone();

            // apply change to active
            let index = cmd::new::Node::new("cat".to_string(), format!("al;sdkfj;asdlfj"))
                .execute(&mut state)
                .unwrap();

            // sync backup and active copy by cloning
            state.backup = state.active.clone();

            cmd::remove::Node::new(index).execute(&mut state).unwrap();
        })
    });
}

/// Investigate best path forward for undo/redo by comparing state update methods
fn swap_diff(c: &mut Criterion) {
    let mut state = EditorState::new(DialogueTreeData::default());
    cmd::new::Project::new("copy_vs_diff".to_string(), true)
        .execute(&mut state)
        .unwrap();
    cmd::new::Name::new(
        KeyString::from("cat").unwrap(),
        NameString::from("Behemoth").unwrap(),
    )
    .execute(&mut state)
    .unwrap();
    assert_eq!(state.active.name_table.get("cat").unwrap(), "Behemoth");

    // add a bunch of nodes
    for i in 0..10000 {
        cmd::new::Node::new("cat".to_string(), format!("{}", i))
            .execute(&mut state)
            .unwrap();
    }

    // placeholder variables for redo
    let mut future_list = Vec::<usize>::with_capacity(1);
    let mut future_stack = Vec::<u8>::with_capacity(1000);

    // sync state beforehand
    state.backup = state.active.clone();

    c.bench_function("swap_diff", |b| {
        b.iter(|| {
            // make a modification to the tree
            cmd::new::Node::new("cat".to_string(), format!("sdkafj"))
                .execute(&mut state)
                .unwrap();

            // diff active -> backup (undo)
            cmd::util::push_diff(
                &state.active,
                &state.backup,
                &mut state.history,
                &mut state.stack,
            )
            .unwrap();
        })
    });

    // diff active -> backup (undo)
    cmd::util::push_diff(
        &state.active,
        &state.backup,
        &mut state.history,
        &mut state.stack,
    )
    .unwrap();

    // diff backup -> active (redo)
    cmd::util::push_diff(
        &state.backup,
        &state.active,
        &mut future_list,
        &mut future_stack,
    )
    .unwrap();

    // sync backup and active copy by applying redo diff
    cmd::util::pop_diff(&mut state.backup, &mut future_list, &mut future_stack).unwrap();

    // apply undo twice to both active and backup
    cmd::util::pop_diff(&mut state.active, &mut state.history, &mut state.stack).unwrap();
    cmd::util::pop_diff(&mut state.backup, &mut state.history, &mut state.stack).unwrap();
}

criterion_group!(benches, copy_diff, swap_diff);
criterion_main!(benches);
