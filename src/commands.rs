//! The command palette: one searchable list of every command, the way VS
//! Code's Show All Commands (Ctrl+Shift+P) offers them.
//!
//! Every command is already an Action (see [`crate::actions`]), so the palette
//! adds no new dispatch path — it only groups the commands the way VS Code
//! does and hands each one to `CommandItem`, which draws the command's current
//! key binding beside it.

use gpui_kit::{
    assets::IconName,
    component::{
        Icon,
        command::{CommandGroup, CommandItem},
    },
};

use crate::{
    actions::*,
    settings::{Appearance, Layout},
};

/// An item with a label, a type icon and the action it dispatches.
fn item(label: &str, icon: IconName, action: impl gpui_kit::Action + 'static) -> CommandItem {
    CommandItem::new()
        .label(label.to_string())
        .icon(Icon::new(icon))
        .action(Box::new(action))
}

/// The commands the palette lists, grouped as VS Code groups its own. The
/// first group is what a VS Code user reaches for most, so it leads.
pub fn groups() -> Vec<CommandGroup> {
    let file = CommandGroup::new()
        .label("File")
        .item(item("New file", IconName::FilePlus, NewFile))
        .item(item("Open file…", IconName::FolderOpen, Open))
        .item(item("Open folder…", IconName::FolderOpen, OpenFolder))
        .item(item("Save", IconName::Save, Save))
        .item(item("Save as…", IconName::Save, SaveAs))
        .item(item("Export as HTML…", IconName::FileOutput, ExportHtml))
        .item(item("Export as PDF…", IconName::FileOutput, ExportPdf))
        .item(item(
            "Export as Word document…",
            IconName::FileOutput,
            ExportDocx,
        ))
        .item(item("Close editor", IconName::X, CloseDocument))
        .item(item("Close other tabs", IconName::X, CloseOtherTabs))
        .item(item(
            "Close tabs to the right",
            IconName::X,
            CloseTabsToTheRight,
        ))
        .item(item("Close window", IconName::X, CloseWindow))
        .item(item("Quit Malgel", IconName::LogOut, Quit));

    let edit = CommandGroup::new()
        .label("Edit")
        .item(item("Copy as rich text", IconName::Clipboard, CopyRichText))
        .item(item("Tidy table", IconName::List, TidyTable));

    let go = CommandGroup::new()
        .label("Go")
        .item(item("Show all commands", IconName::Command, CommandPalette))
        .item(item("Go to file…", IconName::FileText, QuickOpen))
        .item(item("Go to heading…", IconName::Heading1, GoToHeading))
        .item(item("Go to line…", IconName::ArrowRight, GoToLine));

    let view = CommandGroup::new()
        .label("View")
        .item(item("Editor", IconName::PenLine, SetLayout(Layout::Editor)))
        .item(item(
            "Editor and preview",
            IconName::Columns2,
            SetLayout(Layout::Split),
        ))
        .item(item("Preview", IconName::Eye, SetLayout(Layout::Preview)))
        .item(item("Tabs", IconName::ListTree, ToggleTabs))
        .item(item("File sidebar", IconName::PanelLeft, ToggleFileSidebar))
        .item(item("Outline", IconName::ListTree, ToggleOutline))
        .item(item("Focus mode", IconName::Maximize2, ToggleFocusMode))
        .item(item("Toggle word wrap", IconName::TextWrap, ToggleSoftWrap))
        .item(item(
            "Line numbers",
            IconName::ListOrdered,
            ToggleLineNumbers,
        ))
        .item(item(
            "Sync scrolling",
            IconName::ArrowLeftRight,
            ToggleScrollSync,
        ))
        .item(item(
            "Check spelling",
            IconName::SpellCheck,
            ToggleSpellCheck,
        ))
        .item(item(
            "Restore session",
            IconName::Clock,
            ToggleRestoreSession,
        ))
        .item(item(
            "Toggle light and dark",
            IconName::Moon,
            ToggleAppearance,
        ))
        .item(item("Open terminal", IconName::Terminal, SummonTerminal))
        .item(item("Zoom in", IconName::ZoomIn, ZoomIn))
        .item(item("Zoom out", IconName::ZoomOut, ZoomOut))
        .item(item("Actual size", IconName::ScanText, ZoomReset));

    let format = CommandGroup::new()
        .label("Format")
        .item(item("Bold", IconName::Bold, Bold))
        .item(item("Italic", IconName::Italic, Italic))
        .item(item(
            "Strikethrough",
            IconName::Strikethrough,
            Strikethrough,
        ))
        .item(item("Inline code", IconName::Code, InlineCode))
        .item(item("Link", IconName::Link, InsertLink))
        .item(item("Code block", IconName::Code, CodeBlock))
        .item(item("Quote", IconName::Quote, Quote))
        .item(item("Bulleted list", IconName::List, BulletList))
        .item(item("Numbered list", IconName::ListOrdered, NumberedList))
        .item(item("Task list", IconName::ListTodo, TaskList))
        .item(item("Heading 1", IconName::Heading1, Heading(1)))
        .item(item("Heading 2", IconName::Heading2, Heading(2)))
        .item(item("Heading 3", IconName::Heading3, Heading(3)))
        .item(item("Heading 4", IconName::Heading4, Heading(4)))
        .item(item("Heading 5", IconName::Heading5, Heading(5)))
        .item(item("Heading 6", IconName::Heading6, Heading(6)));

    let settings = CommandGroup::new()
        .label("Settings")
        .item(item("Settings…", IconName::Settings, OpenSettings))
        .item(item(
            "Appearance: follow the system",
            IconName::Sun,
            SetAppearance(Appearance::System),
        ))
        .item(item(
            "Appearance: light",
            IconName::Sun,
            SetAppearance(Appearance::Light),
        ))
        .item(item(
            "Appearance: dark",
            IconName::Moon,
            SetAppearance(Appearance::Dark),
        ));

    let help = CommandGroup::new()
        .label("Help")
        .item(item(
            "Markdown guide",
            IconName::BookOpen,
            OpenMarkdownGuide,
        ))
        .item(item("About Malgel", IconName::Info, About));

    vec![file, edit, go, view, format, settings, help]
}
