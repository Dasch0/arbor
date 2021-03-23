use arbor_core::cmd::Executable;
use arbor_core::*;

fn main() {
    let mut cmd_buf = String::with_capacity(1000);

    let mut state = EditorState::new(DialogueTreeData::default());
    loop {
        // print default header
        println!("------------");
        println!("project: {}", state.act.name);
        println!("------------");

        cmd::util::prompt_input(&mut cmd_buf);

        let cmds = shellwords::split(&cmd_buf).unwrap();
        let cmd_result = cmd::Parse::from_iter_safe(cmds);

        // Handle results/errors
        match cmd_result {
            Ok(v) => match v.execute(&mut state) {
                Ok(_r) => println!("success"),
                // errors from arbor operations
                Err(f) => {
                    // pretty print top level error message
                    println!("\u{1b}[1;31merror:\u{1b}[0m {}", f);
                }
            },
            // errors from CLI interface
            Err(e) => println!("{}", e),
        }

        // clear input buffers before starting next input loop
        state.scratchpad.clear();
        cmd_buf.clear();
    }
}
