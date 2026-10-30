//! Pure UI contracts are also run in the x86 bench. No game/window needed.
use mod_ui::{
    flow::{Action, App, Effect, Goal, Screen},
    locale::Language,
    view::{Kind, Transform, View},
};
#[test]
fn financial_choice_and_cancellation_are_preserved_across_screens() {
    let mut app = App::preview(Screen::Host, Language::PtBr);
    app.goal = "2.500.000,50".into();
    assert_eq!(
        app.host().unwrap().goal,
        Goal::Financial {
            minor_units: 250000050
        }
    );
    app.act(Action::ToggleUnlimited);
    assert_eq!(app.host().unwrap().goal, Goal::Unlimited);
    let Some(Effect::Host { request, .. }) = app.act(Action::SubmitHost) else {
        panic!("host intent")
    };
    assert_eq!(app.screen, Screen::Loading);
    app.act(Action::Back);
    assert!(!app.complete(request, true));
    assert!(!app.connected);
    assert!(app.unlimited);
    assert_eq!(app.goal, "2.500.000,50");
}
#[test]
fn disabled_goal_cannot_receive_clicks_in_wide_viewport() {
    let mut app = App::preview(Screen::Host, Language::PtBr);
    app.unlimited = true;
    let view = View::build(&app);
    let goal = view
        .widgets
        .iter()
        .find(|w| {
            matches!(
                w.kind,
                Kind::Input {
                    field: mod_ui::flow::Field::Goal
                }
            )
        })
        .unwrap();
    let t = Transform::new(1920, 1080).unwrap();
    let (x, y) = t.logical(
        (t.x + (goal.rect.x + 10) as f64 * t.scale) as i32,
        (t.y + (goal.rect.y + 10) as f64 * t.scale) as i32,
    );
    assert!(view.hit(x, y).is_none());
    assert!(view.hit(-1.0, 10.0).is_none());
}
