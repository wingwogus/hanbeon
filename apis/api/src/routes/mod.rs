use std::collections::HashMap;

use vespera::axum::Json;

#[vespera::route(get, path = "/health")]
pub async fn health() -> Json<HashMap<String, String>> {
    Json(HashMap::from([("status".to_string(), "ok".to_string())]))
}
