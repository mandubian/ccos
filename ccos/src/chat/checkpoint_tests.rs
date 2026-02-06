#[cfg(test)]
mod tests {
    use crate::chat::checkpoint::Checkpoint;
    use serde_json::json;

    #[test]
    fn test_checkpoint_serialization() {
        let env = json!({"key": "value"});
        let ckpt = Checkpoint::new("run-123".to_string(), env.clone(), 42);

        let serialized = serde_json::to_string(&ckpt).unwrap();
        let deserialized: Checkpoint = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.run_id, "run-123");
        assert_eq!(deserialized.env, env);
        assert_eq!(deserialized.ir_pos, 42);
        assert!(deserialized.pending_yield.is_none());
    }
}
