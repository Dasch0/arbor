

#[repr(fn())]
enum ChoiceKind {
    None,
    SetName = set_name,
}

pub fn set_name() {}
