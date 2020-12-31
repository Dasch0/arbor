use std::io;
use std::io::Write;

static UNKNOWN: &str = "unknown command, type help for more info";
static NONAME: &str = "no name provided";
static HELP: &str = "Arbor
    A tree based dialogue editor

    commands:
    help - display this help menu
    new - create a new project, node, or edge
    project - get the current project info
    node - get the current node info

    exit application:
    q 
    quit 
    exit";

pub struct State {
    project: ropey::Rope,
    project_name: String,

}

mod cmd {
    use super::*;
    
    pub fn help(cmd_iter: &mut std::str::Split<&str>,
                state: &mut State, 
    ) {
        let _ = cmd_iter;
        println!("{}", HELP);
    }

    pub fn new(cmd_iter: &mut std::str::Split<&str>,
               state: &mut State,
    ) {
        match cmd_iter.next() {
            Some("project") => new::project(cmd_iter, state),
            Some("node") => new::node(cmd_iter),
            Some("edge") => new::edge(cmd_iter),
            _ => println!("{}", UNKNOWN)
        }
    }

    mod new {
        use super::*;

        pub fn project(cmd_iter: &mut std::str::Split<&str>, state: &mut State) {
            // create a new text file.
            let project_name = cmd_iter.next();
            match project_name {
                None => {println!("{}", NONAME); return}
                _ =>{}
            }
            // TODO: use openoptions to not overwrite
            let project_file = std::fs::File::create(project_name.unwrap()).unwrap();
            // drop project to save file
            drop(project_file);
            // Open text file as a rope
            let mut project = ropey::Rope::from_reader(
                std::io::BufReader::new(
                    std::fs::File::open(project_name.unwrap()).unwrap()
                )
            ).unwrap();

            // save info to state
            state.project_name = project_name.unwrap().to_string();
            state.project = project;
        }

        pub fn node(cmd_iter: &mut std::str::Split<&str>) {
            let _ = cmd_iter;
        }

        pub fn edge(cmd_iter: &mut std::str::Split<&str>) {
            let _ = cmd_iter;
        }
    }
}

fn main() {
    let mut buf = String::with_capacity(100);    
    println!("welcome to arbor!");

    // TODO: clean up state init
    let mut state = State {
        project: ropey::Rope::new(),
        project_name: String::new(),
    };

    loop {
        print!(">> ");
        io::stdout().flush().unwrap();
        io::stdin()
            .read_line(&mut buf)
            .expect("Failed to read line");
        
        let mut cmd_iter = buf
            .trim_end_matches("\n")
            .split(" "); 

        match cmd_iter.next().unwrap() {
            "help" => cmd::help(&mut cmd_iter, &mut state),
            "new" => cmd::new(&mut cmd_iter, &mut state),
            "q" => break,
            "exit" => break, 
            "quit" => break,
            _ => println!("{}", UNKNOWN),
        }
        buf.clear();
    }
}
