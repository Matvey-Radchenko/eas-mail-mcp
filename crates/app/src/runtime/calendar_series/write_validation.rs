use eas_mail_protocol::{CalendarApplication, protocol};

use super::edit::{EditPlan, ItemAction};
use crate::{AppError, ErrorCode, Result};

pub(super) fn validate(plan: &EditPlan, old: &CalendarApplication) -> Result<()> {
    for step in &plan.steps {
        let result = match &step.action {
            ItemAction::Create(event) => protocol::build_calendar_add(
                "preflight",
                "key",
                "item",
                &event.mutation.application,
            ),
            ItemAction::Update(event) => protocol::build_calendar_change(
                "preflight",
                "key",
                "item",
                &protocol::calendar_change_delta(&old.properties, &event.mutation.application),
            ),
            ItemAction::Delete => continue,
        };
        result.map_err(|error| AppError::new(ErrorCode::ValidationFailed, error.to_string()))?;
    }
    Ok(())
}
