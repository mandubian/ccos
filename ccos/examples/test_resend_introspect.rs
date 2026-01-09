use ccos::ops::introspection_service::IntrospectionService;

#[tokio::main]
async fn main() {
    let spec_url = "https://raw.githubusercontent.com/resend/resend-openapi/main/resend.yaml";
    println!("Testing is_openapi_url: {}", IntrospectionService::is_openapi_url(spec_url));
    
    let service = IntrospectionService::new();
    println!("Calling introspect_openapi...");
    match service.introspect_openapi(spec_url, "Resend Email API").await {
        Ok(result) => {
            println!("Success: {}", result.success);
            if let Some(api) = &result.api_result {
                println!("Title: {}", api.api_title);
                println!("Endpoints: {}", api.endpoints.len());
            }
            if let Some(err) = &result.error {
                println!("Error: {}", err);
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
