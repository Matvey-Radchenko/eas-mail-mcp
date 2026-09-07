use clap::Parser;

use super::super::calendar_args::CalendarCommand;
use super::*;

#[derive(Parser)]
struct Arguments {
    #[command(subcommand)]
    command: CalendarCommand,
}

#[test]
fn recurring_flags_share_slot_constraints_and_require_weekday() -> anyhow::Result<()> {
    let args = Arguments::try_parse_from([
        "calendar",
        "recurring-slots",
        "--weekday",
        "tue",
        "--participant",
        "x@y",
        "--from",
        "2026-09-01",
        "--to",
        "2026-10-01",
        "--time-zone",
        "UTC",
        "--working-hours",
        "tue@09:00-18:00",
        "--duration",
        "60",
        "--buffer",
        "15",
    ])?;
    let CalendarCommand::RecurringSlots(args) = args.command else {
        anyhow::bail!("expected recurring slots")
    };
    let input = recurring_slots(args)?;
    assert!(matches!(input.weekday, ScheduleWeekday::Tue));
    assert_eq!(input.schedule.buffer_minutes, 15);
    assert!(input.schedule.participant_options.is_empty());
    Ok(())
}

#[test]
fn recurring_json_preserves_per_participant_options() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("slots.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "participants":["x@y"], "date_from":"2026-09-01", "date_to":"2026-10-01", "time_zone":"UTC",
            "working_hours":[{"weekdays":["tue"],"start":"09:00","end":"18:00"}],"duration_minutes":60,
            "weekday":"tue", "buffer_minutes":15, "participant_options":[{"input":"x@y","time_zone":"Europe/Belgrade"}]
        }))?,
    )?;
    let args = Arguments::try_parse_from([
        "calendar",
        "recurring-slots",
        "--input",
        &path.to_string_lossy(),
    ])?;
    let CalendarCommand::RecurringSlots(args) = args.command else {
        anyhow::bail!("expected recurring slots")
    };
    let input = recurring_slots(args)?;
    assert_eq!(input.schedule.participant_options.len(), 1);
    assert_eq!(input.schedule.buffer_minutes, 15);
    Ok(())
}
