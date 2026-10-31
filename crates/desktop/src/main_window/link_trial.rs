//! Test a link (D72.5, D72.7): which queue the rules send a link to and why, a switched-off queue
//! that would have matched it included. The core matches, so this and the downloads never
//! disagree.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::Confirm;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Window, div,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::rules::{DraftRules, LinkMatch, LinkTrial as Trial};
use wirecatch_shell::api;

use crate::backend::Backend;
use crate::library::Library;
use crate::text;
use crate::ui::{Role, Typed as _};

/// One queue's rules as its editor has them, tried in place of the saved ones, and that queue's
/// name as typed.
pub struct Draft {
    pub rules: DraftRules,
    pub name: String,
}

/// Reads the draft each time a link is tried.
pub type Drafted = Rc<dyn Fn(&App) -> Option<Draft>>;

/// What Test a link found: where the link goes and why, or why it could not be tried.
enum Answer {
    Lines(Vec<String>),
    Problem(String),
}

pub struct LinkTrial {
    library: Entity<Library>,
    /// `None` tries the saved rules.
    draft: Option<Drafted>,
    link: Entity<InputState>,
    answer: Option<Answer>,
    trying: bool,
    _typing: Subscription,
}

impl LinkTrial {
    pub fn new(
        library: Entity<Library>,
        draft: Option<Drafted>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let link =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://example.com/file.zip"));
        let typing = cx.subscribe_in(
            &link,
            window,
            |trial: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => trial.forget(cx),
                InputEvent::PressEnter { .. } => trial.try_link(window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        Self {
            library,
            draft,
            link,
            answer: None,
            trying: false,
            _typing: typing,
        }
    }

    /// An answer is for the link and the rules it was given: a change to either takes it away.
    pub fn forget(&mut self, cx: &mut Context<'_, Self>) {
        if self.answer.take().is_some() {
            cx.notify();
        }
    }

    fn try_link(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let url = self.link.read(cx).value().trim().to_owned();
        if url.is_empty() || self.trying {
            return;
        }
        let draft = self.draft.as_ref().and_then(|draft| draft(cx));
        let trial = Trial {
            url,
            draft: draft.as_ref().map(|draft| draft.rules.clone()),
        };
        self.trying = true;
        cx.notify();
        let tried = Backend::run(
            cx,
            move |manager| async move { api::try_link(&manager, &trial) },
        );
        cx.spawn_in(window, async move |trial, cx| {
            let found = match tried.await {
                Ok(Ok(result)) => result.map_err(|error| error.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = trial.update(cx, |trial, cx| {
                trial.trying = false;
                trial.answer = Some(match found {
                    Ok(found) => Answer::Lines(trial.lines(&found, draft.as_ref(), cx)),
                    Err(problem) => Answer::Problem(problem),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Where the link goes and why, then the switched-off queue that would have matched it.
    fn lines(&self, found: &LinkMatch, draft: Option<&Draft>, cx: &App) -> Vec<String> {
        let library = self.library.read(cx);
        let name = |queue: Option<QueueId>| {
            if let Some(draft) = draft
                && (queue.is_none() || queue == draft.rules.queue)
            {
                return if draft.name.is_empty() {
                    "This queue".to_owned()
                } else {
                    draft.name.clone()
                };
            }
            queue
                .and_then(|id| library.queue(id))
                .map_or_else(|| "A deleted queue".to_owned(), |q| q.settings.name.clone())
        };
        let mut lines = vec![match &found.matched {
            Some(matched) => format!(
                "Queue: {}. Its rules match {}.",
                name(matched.queue),
                text::rule_phrase(&matched.rule)
            ),
            None => format!(
                "Queue: {}. No queue's rules match it.",
                library
                    .queues
                    .iter()
                    .find(|queue| queue.is_default)
                    .map_or("Main", |queue| queue.settings.name.as_str())
            ),
        }];
        if let Some(off) = &found.switched_off {
            lines.push(format!(
                "{} would match {}, but its rules are off.",
                name(off.queue),
                text::rule_phrase(&off.rule)
            ));
        }
        lines
    }
}

impl Render for LinkTrial {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let danger = cx.theme().danger;
        let answer = match &self.answer {
            Some(Answer::Lines(lines)) => {
                v_flex()
                    .gap_0p5()
                    .children(lines.iter().enumerate().map(|(index, line)| {
                        div()
                            .text_role(
                                if index == 0 {
                                    Role::Body
                                } else {
                                    Role::Secondary
                                },
                                cx,
                            )
                            .child(line.clone())
                    }))
            }
            Some(Answer::Problem(problem)) => v_flex().child(
                div()
                    .text_role(Role::Body, cx)
                    .text_color(danger)
                    .child(problem.clone()),
            ),
            None => v_flex(),
        };
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            // Enter tries the link; let through, a dialog's Confirm would save it.
                            .on_action(|_: &Confirm, _, _| {})
                            .child(Input::new(&self.link)),
                    )
                    .child(
                        Button::new("try-link")
                            .label("Try")
                            .loading(self.trying)
                            .disabled(self.link.read(cx).value().trim().is_empty())
                            .on_click(cx.listener(|trial, _, window, cx| {
                                trial.try_link(window, cx);
                            })),
                    ),
            )
            .child(answer)
    }
}
