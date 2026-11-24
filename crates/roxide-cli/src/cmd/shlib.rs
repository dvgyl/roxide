use roxide_host::NativeHost;

pub(super) fn run(args: &[String]) -> i32 {
    roxide_pkg::shlib::run_cli(args, &mut NativeHost::new())
}
