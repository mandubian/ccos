//! Tests for ops module

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_server_list() {
        let result = server::list_servers().await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.count, output.servers.len());
    }

    #[tokio::test]
    async fn test_config_show() {
        let config_path = PathBuf::from("agent_config.toml");
        let result = config::show_config(config_path).await;
        assert!(result.is_ok());
        let config_info = result.unwrap();
        assert!(config_info.is_valid);
    }

    #[tokio::test]
    async fn test_approval_pending() {
        let result = approval::list_pending().await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.count, output.items.len());
    }
}