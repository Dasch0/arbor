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

mod tree_tests {
    use arbor_core::*;
    #[test]
    fn outgoing_edges() {
        let mut tree = tree::Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0), Position::default());
        let choice = Choice::new(Section::new([0, 0], 0), ReqKind::No, EffectKind::No);

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

    /// Test adding, removing, then re-inserting nodes
    #[test]
    fn add_remove_node() {
        let mut tree = tree::Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0), Position::default());

        for _ in 0..10 {
            tree.add_node(dia).unwrap();
        }

        let tree_full = tree.clone();

        let removed_node = tree.remove_node(5).unwrap();
        tree.insert_node(removed_node, 5).unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let removed_node = tree.remove_node(9).unwrap();
        tree.insert_node(removed_node, 9).unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let removed_node = tree.remove_node(0).unwrap();
        tree.insert_node(removed_node, 0).unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));
    }

    /// Test adding, removing, then re-inserting edges
    #[test]
    fn add_remove_edge() {
        let mut tree = tree::Tree::with_capacity(10, 10);
        //dummy dialogue for creating nodes
        let dia = Dialogue::new(Section::new([0, 0], 0), Position::default());
        let choice = Choice::new(Section::new([0, 0], 0), ReqKind::No, EffectKind::No);

        for _ in 0..10 {
            tree.add_node(dia).unwrap();
        }

        // add edges such that all edges are an outgoing edge of node 0
        for i in 0..10 {
            tree.add_edge(0, i, choice).unwrap();
        }
        let tree_full = tree.clone();

        let (source, target, removed_edge, placement) = tree.remove_edge(5).unwrap();

        tree.insert_edge(source, target, removed_edge, 5, placement)
            .unwrap();
        assert_eq!(format!("{:#?}", tree), format!("{:#?}", tree_full),);

        let (source, target, removed_edge, placement) = tree.remove_edge(0).unwrap();
        tree.insert_edge(source, target, removed_edge, 0, placement)
            .unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let (source, target, removed_edge, placement) = tree.remove_edge(9).unwrap();
        tree.insert_edge(source, target, removed_edge, 9, placement)
            .unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));

        let (source5, target5, removed_edge5, placement5) = tree.remove_edge(5).unwrap();
        let (source0, target0, removed_edge0, placement0) = tree.remove_edge(0).unwrap();
        // index shifted by 2 because of prior removals
        let (source9, target9, removed_edge9, placement9) = tree.remove_edge(7).unwrap();
        tree.insert_edge(source9, target9, removed_edge9, 7, placement9)
            .unwrap();
        tree.insert_edge(source0, target0, removed_edge0, 0, placement0)
            .unwrap();
        tree.insert_edge(source5, target5, removed_edge5, 5, placement5)
            .unwrap();
        assert_eq!(format!("{:?}", tree), format!("{:?}", tree_full));
    }
}
