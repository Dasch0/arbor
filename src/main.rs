use std::io;
use std::io::Write;

static UNKNOWN: &str = "unknown command, type help for more info";

mod cmd {
    use super::*;
    
    pub fn help(cmd_iter: &mut std::str::Split<&str>) {
        let _ = cmd_iter;
        println!(
            "
            Arbor
            A tree based dialogue editor
            
            commands:
            help - display this help menu

            q 
            quit 
            exit 
            "
        );
    }

    pub fn new(cmd_iter: &mut std::str::Split<&str>) {
        match cmd_iter.next() {
            Some("project") => new::project(cmd_iter),
            Some("node") => new::node(cmd_iter),
            Some("edge") => new::edge(cmd_iter),
            _ => println!("{}", UNKNOWN)
        }
    }

    mod new {
        pub fn project(cmd_iter: &mut std::str::Split<&str>) {
            let _test = ropey::Rope::from_str("a;falkj;jgdflkkljdlsfkjlkslakjslaksjflsdjlkfjlaksjlaskfjak");

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
            "help" => cmd::help(&mut cmd_iter),
            "new" => cmd::new(&mut cmd_iter),
            "q" => break,
            "exit" => break, 
            "quit" => break,
            _ => println!("{}", UNKNOWN),
        }
        buf.clear();
    }
}
