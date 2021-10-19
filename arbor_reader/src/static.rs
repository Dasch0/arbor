/// Thread safe static cell with zero cost to access data once initialized
use std::{cell::UnsafeCell, marker::PhantomData, mem::MaybeUninit, sync::Once};

pub struct TaggedOnceCell<T, Tag> {
    once: Once,
    tag: PhantomData<Tag>,
    data: UnsafeCell<MaybeUninit<T>>,
}

/// A marker proving that the unique cell with tag `Tag` is initialized.
// This cannot be sent across threads, the only way to obtain it is by running
// get_or_init() in the current thread
#[derive(Clone, Copy)]
pub struct Init<Tag>(PhantomData<Tag>);

impl<T, Tag> TaggedOnceCell<T, Tag> {
    /// Make an uninitialized cell.
    /// This must only be called once for each `Tag` type.
    pub const fn new() -> Self {
        TaggedOnceCell {
            data: UnsafeCell::new(MaybeUninit::<T>::uninit()),
            tag: PhantomData,
            once: Once::new(),
        }
    }

    /// Initialize the TaggedOnceCell. This function attempts to initialize the cell if it is not
    /// already initialized using the provIDed fn(). This method returns a ZST 'Tag' which is required
    /// to gain access to the underlying data after init.
    ///
    /// Each thread accessing a TaggedOnceCell should call init() to obtain the tag, the initialization
    /// code will only run once.
    pub fn init<F>(&self, f: F) -> Init<Tag>
    where
        F: Fn() -> T,
    {
        unsafe {
            self.once.call_once(|| {
                let mut_data = &mut *self.data.get();
                mut_data.write(f());
            });
        }
        Init(self.tag)
    }

    #[inline(never)]
    pub fn get(&self, _: Init<Tag>) -> &T {
        // SAFETY: Init tag proves that `get_or_init` has successfully
        // returned before in the current thread, initializing the cell.
        unsafe {
            let maybe_val = &mut *self.data.get();
            maybe_val.assume_init_ref()
        }
    }
}

macro_rules! tagged_cell {
    (static $name:ident : TaggedOnceCell<$type:ty, _> = TaggedOnceCell::new();) => {
        #[allow(non_snake_case)]
        mod $name {
            #[allow(dead_code)]
            pub struct NewType;
        }

        static $name: TaggedOnceCell<$type, self::$name::NewType> = TaggedOnceCell::new();
    };
}
