use std::sync::LazyLock;

pub struct EnvVars {
    // TODO: make SocketAddr
    pub host: String,
    pub port: u16,
    pub log_dir: String,
}

pub static ENV_VARS: LazyLock<EnvVars> = LazyLock::new(|| {
    dotenvy::dotenv().ok();
    EnvVars {
        host: dotenvy::var("HOST").unwrap_or("0.0.0.0".to_string()),
        port: {
            dotenvy::var("PORT")
                .unwrap_or(6969.to_string())
                .parse::<u16>()
                .expect("PORT Env Var has to be a 16 Bit unsigned integer")
        },
        log_dir: dotenvy::var("LOG_DIR").unwrap_or("./".to_string()),
    }
});
