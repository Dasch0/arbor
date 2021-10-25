use arbor_core::{editor::Editor, EffectKind, ReqKind, Result};
use eframe::egui::Pos2;
use rand::Rng;

static TEXT: &str = "
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Donec rutrum nunc at nulla iaculis tempor.
Donec ut magna at orci mattis accumsan. Nulla mattis pulvinar congue. Mauris ac lectus velit. 
Aliquam erat volutpat. Cras suscipit risus eget magna semper, id condimentum orci dignissim. 
Quisque a leo quis justo tincidunt condimentum. Nam erat justo, gravida et odio a, blandit 
tincidunt est. Vivamus elementum molestie nibh, ut eleifend nisl. Duis lobortis maximus congue. 
Donec eleifend nec nunc a ultricies. Aenean id consequat tellus, eu egestas arcu. Cras lorem nulla,
fringilla et ullamcorper sed, ultricies a enim. Sed sit amet orci lectus. Pellentesque turpis tellus,
commodo at ultrices sit amet, maximus et dui. Aenean ut lectus pulvinar, sagittis justo ac, 
efficitur lorem. Etiam viverra faucibus lectus viverra sagittis. Vestibulum efficitur metus bibendum
ornare luctus. Phasellus magna eros, faucibus nec quam in, pretium vestibulum eros. Nunc elit ex,
sollicitudin vel felis ac, pretium cursus lacus. Duis placerat erat ut felis consectetur finibus.
Duis dignissim dapibus lobortis. Vestibulum rutrum elit ac nulla porttitor, at interdum mi vestibulum.
";

/// Helper function that creates a giant random tree, mainly going to be used for dev purposes
pub fn lorem_ipsum(position_table: &mut Vec<Pos2>, count: usize) -> Result<Editor> {
    // spin up rng
    let mut rng = rand::thread_rng();
    // create new project
    let mut editor = Editor::new("lorem_ipsum", None)?;
    // reset position table
    position_table.clear();

    let key = "author";
    editor.new_name(key, "Cicero")?;

    // create a ton of nodes
    for i in 0..count {
        // select random selection of lorem_ipsum text
        let text_len = rng.gen_range(10..256);
        let text_start = rng.gen_range(0..(TEXT.len() - text_len));
        let text_end = text_start + text_len;

        // bias so later nodes have generally higher valued positions
        let bias = i as f32 * 0.1;

        let pos = Pos2::new(
            rng.gen_range(bias - 1.0..bias + 1.0),
            rng.gen_range(bias - 1.0..bias + 1.0),
        );
        let idx = editor.new_node(key, &TEXT[text_start..text_end]).unwrap();
        // verify length of position_table
        assert_eq!(position_table.len(), idx);
        position_table.push(pos);
    }

    // create a ton of edges
    for _ in 0..count {
        // select random selection of lorem_ipsum text
        let text_len = rng.gen_range(10..256);
        let text_start = rng.gen_range(0..(TEXT.len() - text_len));
        let text_end = text_start + text_len;

        let start = rng.gen_range(0..count);
        let end = rng.gen_range(start..count);

        let _idx = editor.new_edge(
            &TEXT[text_start..text_end],
            start,
            end,
            ReqKind::No,
            EffectKind::No,
        );
    }

    // return the editor for the ui to use
    Ok(editor)
}
