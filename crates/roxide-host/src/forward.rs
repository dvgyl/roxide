/// Implement [`Host`](crate::Host) for a wrapper by forwarding calls to a field
/// that implements the trait. Supply optional method definitions to override
/// individual calls.
///
/// ```
/// use roxide_host::{Host, PureHost, forward_host};
///
/// struct Wrapper {
///     inner: Box<dyn Host>,
/// }
///
/// forward_host!(Wrapper, inner;
///     fn pid(&self) -> u32 {
///         42
///     }
/// );
///
/// let mut wrapper = Wrapper { inner: Box::new(PureHost::new()) };
/// assert_eq!(wrapper.pid(), 42);
/// wrapper.set_env_var("KEY", Some(b"value")).unwrap();
/// assert_eq!(wrapper.env_var("KEY").as_deref(), Some(b"value".as_slice()));
/// ```
#[macro_export]
macro_rules! forward_host {
    ($wrapper:ty, $field:ident $(; $(fn $name:ident $args:tt $(-> $result:ty)? $body:block)*)?) => {
        $crate::forward_host!(@impl ($), $wrapper, [$field], [
            $($(fn $name $args $(-> $result)? $body)*)?
        ]);
    };

    (@impl ($d:tt), $wrapper:ty, $target:tt, [
        $(fn $name:ident $args:tt $(-> $result:ty)? $body:block)*
    ]) => {
        const _: () = {
            macro_rules! emit {
                $(($name, $d method:item) => {};)*
                ($d name:ident, $d method:item) => { $d method };
            }

            impl $crate::Host for $wrapper {
                $crate::forward_host!(@methods $target, emit);
                $(fn $name $args $(-> $result)? $body)*
            }
        };
    };

    (@target $this:ident [$field:ident]) => { $this.$field };
    (@target $this:ident []) => { **$this };

    (@method $field:tt, $emit:ident, [$($mutable:tt)?]
        $name:ident($($arg:ident: $type:ty),*) $(-> $result:ty)?
    ) => {
        $emit!($name, fn $name(&$($mutable)? self $(, $arg: $type)*) $(-> $result)? {
            $crate::Host::$name(&$($mutable)? $crate::forward_host!(@target self $field) $(, $arg)*)
        });
    };

    (@methods $field:tt, $emit:ident) => {
        $crate::forward_host!(@method $field, $emit, [mut] write_out(bytes: &[u8]) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [mut] write_err(bytes: &[u8]) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [mut] flush(stream: $crate::StdStream) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [mut] read_line(buf: &mut ::std::vec::Vec<u8>) -> $crate::HostResult<usize>);
        $crate::forward_host!(@method $field, $emit, [] is_tty(stream: $crate::StdStream) -> bool);
        $crate::forward_host!(@method $field, $emit, [mut] open(path: &::std::path::Path, mode: $crate::OpenMode) -> $crate::HostResult<::std::boxed::Box<dyn $crate::HostFile>>);
        $crate::forward_host!(@method $field, $emit, [] metadata(path: &::std::path::Path, follow: bool) -> $crate::HostResult<$crate::FileMeta>);
        $crate::forward_host!(@method $field, $emit, [] read_dir(path: &::std::path::Path) -> $crate::HostResult<::std::vec::Vec<$crate::DirEntry>>);
        $crate::forward_host!(@method $field, $emit, [mut] create_dir(path: &::std::path::Path, recursive: bool, mode: u32) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [mut] remove(path: &::std::path::Path, recursive: bool) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [mut] rename(from: &::std::path::Path, to: &::std::path::Path) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [] canonicalize(path: &::std::path::Path) -> $crate::HostResult<::std::path::PathBuf>);
        $crate::forward_host!(@method $field, $emit, [] cwd() -> $crate::HostResult<::std::path::PathBuf>);
        $crate::forward_host!(@method $field, $emit, [mut] set_cwd(path: &::std::path::Path) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [] session_temp_dir() -> &::std::path::Path);
        $crate::forward_host!(@method $field, $emit, [] home_dir() -> ::std::option::Option<::std::path::PathBuf>);
        $crate::forward_host!(@method $field, $emit, [] env_var(key: &str) -> ::std::option::Option<::std::vec::Vec<u8>>);
        $crate::forward_host!(@method $field, $emit, [mut] set_env_var(key: &str, value: ::std::option::Option<&[u8]>) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [] env_vars() -> ::std::vec::Vec<(::std::vec::Vec<u8>, ::std::vec::Vec<u8>)>);
        $crate::forward_host!(@method $field, $emit, [] locale() -> $crate::LocaleInfo);
        $crate::forward_host!(@method $field, $emit, [] tzif(zone: &str) -> $crate::HostResult<::std::vec::Vec<u8>>);
        $crate::forward_host!(@method $field, $emit, [] now() -> $crate::WallTime);
        $crate::forward_host!(@method $field, $emit, [] monotonic() -> ::std::time::Duration);
        $crate::forward_host!(@method $field, $emit, [] cpu_times() -> $crate::CpuTimes);
        $crate::forward_host!(@method $field, $emit, [mut] sleep(duration: ::std::time::Duration) -> $crate::HostResult<()>);
        $crate::forward_host!(@method $field, $emit, [] pid() -> u32);
        $crate::forward_host!(@method $field, $emit, [mut] fill_entropy(buf: &mut [u8]));
        $crate::forward_host!(@method $field, $emit, [] sys_info() -> $crate::SysInfo);
        $crate::forward_host!(@method $field, $emit, [mut] spawn(spec: &$crate::CommandSpec) -> $crate::HostResult<$crate::ProcessOutput>);
        $crate::forward_host!(@method $field, $emit, [mut] poll_interrupt() -> bool);
        $crate::forward_host!(@method $field, $emit, [] limits() -> &$crate::Limits);
    };
}

forward_host!(@impl ($), Box<dyn crate::Host + '_>, [], []);

#[cfg(test)]
mod tests {
    use crate::{Host, HostResult, PureHost};

    struct Plain {
        inner: PureHost,
    }

    forward_host!(Plain, inner);

    struct Wrapper {
        inner: PureHost,
        pid: u32,
    }

    forward_host!(Wrapper, inner;
        fn pid(&self) -> u32 {
            self.pid
        }

        fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
            self.inner.write_out(bytes)
        }
    );

    #[test]
    fn forwards_calls_and_accepts_overrides() {
        let mut plain = Plain {
            inner: PureHost::new(),
        };
        let mut wrapper = Wrapper {
            inner: PureHost::new(),
            pid: 42,
        };
        assert_eq!(plain.pid(), plain.inner.pid());
        assert_eq!(wrapper.pid(), 42);
        assert_eq!(wrapper.session_temp_dir(), wrapper.inner.session_temp_dir());

        plain.write_err(b"error").unwrap();
        assert_eq!(plain.inner.stderr_str(), "error");

        wrapper.write_out(b"output").unwrap();
        wrapper.write_err(b" error").unwrap();
        assert_eq!(wrapper.inner.stdout_str(), "output error");
        assert!(wrapper.inner.stderr_str().is_empty());
    }
}
