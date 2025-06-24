use crate::ast::TopLevel;
use crate::error_reporting::ValidationError;
use validator::Validate;

pub fn validate_toplevel(toplevel: &TopLevel) -> Result<(), ValidationError> {
    match toplevel.validate() {
        Ok(_) => Ok(()),
        Err(e) => {
            let type_name = match toplevel {
                TopLevel::Intent(_) => "Intent",
                TopLevel::Plan(_) => "Plan",
                TopLevel::Action(_) => "Action",
                TopLevel::Capability(_) => "Capability",
                TopLevel::Resource(_) => "Resource",
                TopLevel::Module(_) => "Module",
                TopLevel::Expression(_) => "Expression",
            };
            Err(ValidationError::SchemaError {
                type_name: type_name.to_string(),
                errors: e,
            })
        }
    }
}
