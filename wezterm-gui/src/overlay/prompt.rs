use crate::scripting::guiwin::GuiWin;
use config::keyassignment::{KeyAssignment, PromptInputLine};
use mux::tab::TabId;
use mux::termwiztermtab::TermWizTerminal;
use mux::Mux;
use mux_lua::MuxPane;
use std::rc::Rc;
use termwiz::input::{InputEvent, KeyCode, KeyEvent};
use termwiz::lineedit::*;
use termwiz::surface::Change;
use termwiz::terminal::Terminal;

struct PromptHost {
    history: BasicHistory,
}

impl PromptHost {
    fn new() -> Self {
        Self {
            history: BasicHistory::default(),
        }
    }
}

impl LineEditorHost for PromptHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        let (line, _cursor) = editor.get_line_and_cursor();
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            }) => line.is_empty().then_some(Action::Cancel),
            _ => None,
        }
    }
}

pub fn show_line_prompt_overlay(
    mut term: TermWizTerminal,
    args: PromptInputLine,
    window: GuiWin,
    pane: MuxPane,
) -> anyhow::Result<()> {
    let name = match *args.action {
        KeyAssignment::EmitEvent(id) => id,
        _ => anyhow::bail!(
            "PromptInputLine requires action to be defined by wezterm.action_callback"
        ),
    };

    term.no_grab_mouse_in_raw_mode();
    let mut text = args.description.replace("\r\n", "\n").replace("\n", "\r\n");
    text.push_str("\r\n");
    term.render(&[Change::Text(text)])?;

    let mut host = PromptHost::new();
    let mut editor = LineEditor::new(&mut term);
    editor.set_prompt(&args.prompt);
    let line =
        editor.read_line_with_optional_initial_value(&mut host, args.initial_value.as_deref())?;

    promise::spawn::spawn_into_main_thread(async move {
        trampoline(name, window, pane, line);
        anyhow::Result::<()>::Ok(())
    })
    .detach();

    Ok(())
}

/// Unlike PromptHost, escape cancels even when the line has text in it,
/// since the rename prompt starts out holding the current title.
struct RenameTabHost {
    history: BasicHistory,
}

impl LineEditorHost for RenameTabHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        _editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            }) => Some(Action::Cancel),
            _ => None,
        }
    }
}

/// Prompts for a new title for `tab_id`, starting from `current_title`.
/// Submitting an empty line clears the tab title, so that the tab shows
/// the title of its active pane again.
pub fn show_rename_tab_overlay(
    mut term: TermWizTerminal,
    tab_id: TabId,
    current_title: String,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();
    term.render(&[Change::Text(
        "Rename tab (Enter to apply, Esc to cancel, \
         empty to use the program's title)\r\n"
            .to_string(),
    )])?;

    let mut host = RenameTabHost {
        history: BasicHistory::default(),
    };
    let mut editor = LineEditor::new(&mut term);
    editor.set_prompt("> ");
    let line = editor.read_line_with_optional_initial_value(&mut host, Some(&current_title))?;

    if let Some(line) = line {
        promise::spawn::spawn_into_main_thread(async move {
            if let Some(tab) = Mux::get().get_tab(tab_id) {
                tab.set_title(line.trim());
            }
        })
        .detach();
    }

    Ok(())
}

fn trampoline(name: String, window: GuiWin, pane: MuxPane, line: Option<String>) {
    promise::spawn::spawn(async move {
        config::with_lua_config_on_main_thread(move |lua| do_event(lua, name, window, pane, line))
            .await
    })
    .detach();
}

async fn do_event(
    lua: Option<Rc<mlua::Lua>>,
    name: String,
    window: GuiWin,
    pane: MuxPane,
    line: Option<String>,
) -> anyhow::Result<()> {
    if let Some(lua) = lua {
        let args = lua.pack_multi((window, pane, line))?;

        if let Err(err) = config::lua::emit_event(&lua, (name.clone(), args)).await {
            log::error!("while processing {} event: {:#}", name, err);
        }
    }

    Ok(())
}
