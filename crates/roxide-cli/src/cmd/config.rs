use roxide_host::NativeHost;

pub(super) fn run(args: &[String]) -> i32 {
    roxide_pkg::makeconf::run_config_cli(args, &mut NativeHost::new())
}
