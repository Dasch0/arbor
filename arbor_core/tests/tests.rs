use arbor_core::*;

#[allow(dead_code)]
fn setup_logger() {
    simple_logger::SimpleLogger::new().init().unwrap();
}

/// helper function to parse cmd_bufs in the same way the editor does
#[inline(always)]
#[allow(dead_code)]
fn run_cmd(cmd_buf: &str, state: &mut EditorState) -> Result<usize> {
    let cmds = shellwords::split(&cmd_buf).unwrap();
    let res = cmd::Parse::from_iter_safe(cmds);
    let v = res.unwrap();
    v.execute(state)
}

#[test]
/// Test basic use case of the editor, new project, add a few nodes and names, list the output,
/// save the project, reload, list the output again
fn simple() {
    setup_logger();
    let mut cmd_buf = String::with_capacity(1000);
    let mut state = EditorState::new(DialogueTreeData::default());
    cmd_buf.push_str("new project \"simple_test\" -s");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();
    assert_eq!(state.active.name, "simple_test");

    cmd_buf.push_str("new name cat Behemoth");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();
    assert_eq!(state.active.name_table.get("cat").unwrap(), "Behemoth");

    cmd_buf.push_str("new val rus_lit 50");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();
    assert_eq!(*state.active.val_table.get("rus_lit").unwrap(), 50);

    cmd_buf.push_str("new node cat \"Well, who knows, who knows\"");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();
    cmd_buf.push_str(
        "new node cat \"'I protest!' ::cat:: exclaimed hotly. 'Dostoevsky is immortal'\"",
    );
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();
    cmd_buf.push_str("new edge -r Less(rus_lit,51) -e Sub(rus_lit,1) 0 1 \"Dostoevsky's dead\"");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();

    cmd_buf.push_str("list");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();

    let expected_list = concat!(
        "node 0: Behemoth says \"Well, who knows, who knows\"\r\n",
        "--> edge 0 to node 1: \"Dostoevsky's dead\"\r\n",
        "    requirements: Less(\"rus_lit\", 51), effects: Sub(\"rus_lit\", 1)\r\n",
        "node 1: Behemoth says \"'I protest!' Behemoth exclaimed hotly. 'Dostoevsky is immortal'\"\r\n",
    );
    assert_eq!(state.scratchpad, expected_list);
    state.scratchpad.clear();

    cmd_buf.push_str("save");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();

    cmd_buf.push_str("load simple_test");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();

    cmd_buf.push_str("rebuild");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();

    cmd_buf.push_str("list");
    run_cmd(&cmd_buf, &mut state).unwrap();
    cmd_buf.clear();

    assert_eq!(state.scratchpad, expected_list);
    state.scratchpad.clear();

    std::fs::remove_file("simple_test.tree").unwrap();
    std::fs::remove_file("simple_test.tree.bkp").unwrap();
}

#[test]
// TODO: move to criterion
/// Benchmark node parsing worst case, many substitutions and improperly sized buffer
fn stress_parse_node() {
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
    let mut name_buf = String::with_capacity(1);
    let mut buf = String::with_capacity(1);
    cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
}

#[test]
/// Benchmark standard node parsing case, few substitutions and pre-allocated buffer
fn quick_parse_node() {
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
    cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
}

mod tree_tests {
    use arbor_core::*;
    #[test]
    fn outgoing_edges() {
        let mut tree = tree::Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0), Position::default());
        let choice = Choice::new(Section::new([0, 0], 0), ReqKind::None, EffectKind::None);

        for _ in 0..10 {
            tree.add_node(dia).unwrap();
        }

        // add edges such that all edges are an outgoing edge of node 0
        for i in 0..10 {
            tree.add_edge(0, i, choice).unwrap();
        }

        // iterate over all outgoing edges of node 0 and verify they are correct
        let outgoing_edges: Vec<tree::EdgeIndex> = tree.outgoing_from_index(0).unwrap().collect();

        assert_eq!(outgoing_edges, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
