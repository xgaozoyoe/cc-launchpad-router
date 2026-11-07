use mongodb::{bson::doc, Client};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let uri = std::env::var("MONGODB_URI")?;
    if !uri.starts_with("mongodb://127.0.0.1:") {
        return Err("This helper only initializes local development MongoDB".into());
    }
    let client = Client::with_uri_str(uri).await?;
    let db = client.database("admin");
    let hello = db.run_command(doc! {"hello":1}).await?;
    if !hello.contains_key("setName") {
        db.run_command(
            doc! {"replSetInitiate":{"_id":"rs0","members":[{"_id":0,"host":"127.0.0.1:27188"}]}},
        )
        .await?;
    }
    for _ in 0..120 {
        if db
            .run_command(doc! {"hello":1})
            .await?
            .get_bool("isWritablePrimary")
            .unwrap_or(false)
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err("MongoDB did not elect a primary".into())
}
