#[cfg(not(target_os = "linux"))]
mod shim_impl {
    #[allow(dead_code)]
    #[derive(Debug, Copy, Clone)]
    pub enum ArgType {
        Struct,
        Array,
        Variant,
        DictEntry,
        String,
        ObjectPath,
        Signature,
        Unix,
        Boolean,
        Byte,
        Int16,
        UInt16,
        Int32,
        UInt32,
        Int64,
        UInt64,
        Double,
        Invalid,
    }

    #[allow(dead_code)]
    #[derive(Debug, Clone)]
    pub struct Signature;

    impl Signature {
        #[allow(dead_code)]
        pub fn from(_s: &str) -> Self {
            Self
        }
    }

    #[allow(dead_code)]
    pub struct IterAppend;

    pub trait Arg {
        #[allow(dead_code)]
        const ARG_TYPE: ArgType;
        #[allow(dead_code)]
        fn signature() -> Signature;
    }

    pub trait Append {
        #[allow(dead_code)]
        fn append_by_ref(&self, _ia: &mut IterAppend);
    }

    #[allow(dead_code)]
    pub trait RefArg {}
}

#[cfg(not(target_os = "linux"))]
pub use shim_impl::{Append, Arg, ArgType, IterAppend, Signature};

#[cfg(target_os = "linux")]
pub use dbus::arg::{Append, Arg, ArgType, IterAppend, RefArg};
#[cfg(target_os = "linux")]
pub use dbus::Signature;
