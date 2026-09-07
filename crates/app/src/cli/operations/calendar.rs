use super::calendar_args::CalendarCommand;
use super::calendar_input;
use super::output::{self, OutputKind, OutputMode};
use super::writes;
use crate::Runtime;
use crate::cli::CliExit;

pub(super) async fn run(
    runtime: &Runtime,
    command: CalendarCommand,
    mode: OutputMode,
) -> crate::Result<CliExit> {
    match command {
        CalendarCommand::Availability(arguments) => {
            let response =
                runtime.calendar_availability(calendar_input::availability(arguments)?).await;
            output::emit(response, mode, OutputKind::Availability, true)
        }
        CalendarCommand::FindSlots(arguments) => {
            let response =
                runtime.calendar_find_slots(calendar_input::find_slots(arguments)?).await;
            output::emit(response, mode, OutputKind::Slots, true)
        }
        CalendarCommand::RecurringSlots(arguments) => {
            let response = runtime
                .calendar_find_recurring_slots(calendar_input::recurring_slots(arguments)?)
                .await;
            output::emit(response, mode, OutputKind::RecurringSlots, true)
        }
        CalendarCommand::Search(arguments) => {
            let response = runtime.calendar_search(calendar_input::search(arguments)?).await;
            output::emit(response, mode, OutputKind::CalendarList, true)
        }
        CalendarCommand::Agenda(arguments) => {
            let response = runtime.calendar_search(calendar_input::agenda(arguments)?).await;
            output::emit(response, mode, OutputKind::CalendarList, true)
        }
        CalendarCommand::Get(arguments) => {
            let response = runtime.calendar_get(calendar_input::get(arguments)?).await;
            output::emit(response, mode, OutputKind::CalendarEvent, true)
        }
        CalendarCommand::Create(arguments) => {
            let (input, yes) = calendar_input::create(arguments)?;
            writes::calendar_create(runtime, input, yes, mode).await
        }
        CalendarCommand::Update(arguments) => {
            let (input, yes) = calendar_input::update(arguments)?;
            writes::calendar_update(runtime, input, yes, mode).await
        }
        CalendarCommand::Delete(arguments) => {
            let (input, yes) = calendar_input::delete(arguments)?;
            writes::calendar_delete(runtime, input, yes, mode).await
        }
        CalendarCommand::Cancel(arguments) => {
            let (input, yes) = calendar_input::cancel(arguments)?;
            writes::calendar_cancel(runtime, input, yes, mode).await
        }
        CalendarCommand::Respond(arguments) => {
            let (input, yes) = calendar_input::respond(arguments)?;
            writes::calendar_respond(runtime, input, yes, mode).await
        }
    }
}
