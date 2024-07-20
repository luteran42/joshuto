#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub enum LineModeArgs {
    Size,
    ModifyTime,
    AccessTime,
    #[cfg(not(target_env = "musl"))]
    BirthTime,
    User,
    Group,
    Permission,
    #[default]
    Null,
}

impl AsRef<str> for LineModeArgs {
    fn as_ref(&self) -> &str {
        match self {
            LineModeArgs::Size => "size",
            LineModeArgs::ModifyTime => "mtime",
            LineModeArgs::AccessTime => "atime",
            #[cfg(not(target_env = "musl"))]
            LineModeArgs::BirthTime => "ctime",
            LineModeArgs::User => "user",
            LineModeArgs::Group => "group",
            LineModeArgs::Permission => "perm",
            LineModeArgs::Null => unreachable!(),
        }
    }
}
