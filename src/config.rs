/// Configuration data
struct MilterConfig {
    port: u16,
    rejection_message: String,
}

struct ApiConfig {
    port: u16,
}

struct AppConfig {
    milter: Option<MilterConfig>
    api: Option<ApiConfig>
}