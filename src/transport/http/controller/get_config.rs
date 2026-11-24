use crate::globals;
use crate::httpd::RequestCtx;

pub fn get_config_controller(ctx: &mut RequestCtx) {
    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };

    ctx.response.header.set("Content-Type", "application/json");
    ctx.write_string(cfg.to_json());
}
