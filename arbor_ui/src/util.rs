use arbor_core::{cmd, EditorState, Executable, KeyString, NameString, Position, Result};
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
pub fn lorem_ipsum(state: &mut EditorState, count: usize) -> Result<()> {
    // spin up rng
    let mut rng = rand::thread_rng();
    // create new project
    cmd::new::Project::new("lorem_ipsum".into(), true).execute(state)?;

    let key = KeyString::from("author")?;
    cmd::new::Name::new(key, NameString::from("Cicero")?).execute(state)?;

    // create a ton of nodes
    for i in 0..count {
        // select random selection of lorem_ipsum text
        let text_len = rng.gen_range(10..256);
        let text_start = rng.gen_range(0..(TEXT.len() - text_len));
        let text_end = text_start + text_len;

        // bias so later nodes have generally higher valued positions
        let bias = i as f32 * 0.1;

        let pos = Position::new(
            rng.gen_range(bias - 1.0..bias + 1.0),
            rng.gen_range(bias - 1.0..bias + 1.0),
        );
        let idx = cmd::new::Node::new(key.to_string(), TEXT[text_start..text_end].to_string())
            .execute(state)?;
        state.active.tree.get_node_mut(idx)?.pos = pos;
    }

    // create a ton of edges
    for _ in 0..count {
        // select random selection of lorem_ipsum text
        let text_len = rng.gen_range(10..256);
        let text_start = rng.gen_range(0..(TEXT.len() - text_len));
        let text_end = text_start + text_len;

        let start = rng.gen_range(0..count);
        let end = rng.gen_range(start..count);

        let _idx = cmd::new::Edge::new(
            start,
            end,
            TEXT[text_start..text_end].to_string(),
            None,
            None,
        )
        .execute(state)?;
    }

    Ok(())
}
