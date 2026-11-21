use super::*;
use crate::tui::keymap::{KeysFile, Preset};
use ratatui_core::backend::TestBackend;
use ratatui_core::terminal::Terminal;
use ratatui_crossterm::crossterm::event::{KeyCode, KeyModifiers};

fn emacs(text: &str) -> Editor {
    Editor::new(text, Keymap::preset(&SECTION, Preset::Emacs))
}

fn default(text: &str) -> Editor {
    Editor::new(text, Keymap::preset(&SECTION, Preset::Default))
}

/// Press each key written in Emacs notation; a lone character types.
fn press(ed: &mut Editor, keys: &str) -> Outcome {
    let mut last = Outcome::Continue;
    for token in keys.split(' ') {
        let key: crate::tui::Key = token.parse().unwrap_or_else(|e| panic!("{token}: {e}"));
        let mut m = KeyModifiers::NONE;
        if key.ctrl {
            m |= KeyModifiers::CONTROL;
        }
        if key.meta {
            m |= KeyModifiers::ALT;
        }
        if key.shift {
            m |= KeyModifiers::SHIFT;
        }
        last = ed.handle(&KeyEvent::new(key.code, m));
    }
    last
}

fn typed(ed: &mut Editor, text: &str) {
    for c in text.chars() {
        ed.handle(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
}

fn screen(ed: &Editor) -> String {
    let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
    terminal.draw(|f| f.render_widget(ed, f.area())).unwrap();
    terminal.backend().to_string()
}

#[test]
fn typing_inserts_and_saving_returns_the_text_with_one_newline() {
    let mut ed = emacs("");
    typed(&mut ed, "hello");
    press(&mut ed, "RET");
    typed(&mut ed, "world");
    assert!(ed.is_dirty());
    assert_eq!(
        press(&mut ed, "C-x C-s"),
        Outcome::Save("hello\nworld\n".into())
    );
}

#[test]
fn the_with_editor_keys_finish_and_cancel() {
    let mut ed = emacs("a\n");
    assert_eq!(press(&mut ed, "C-c C-c"), Outcome::Finish("a\n".into()));
    assert_eq!(press(&mut ed, "C-x #"), Outcome::Finish("a\n".into()));
    assert_eq!(press(&mut ed, "C-c C-k"), Outcome::Cancel);
}

#[test]
fn save_keeps_editing_and_saved_resets_the_change_check() {
    let mut ed = emacs("a\n");
    typed(&mut ed, "b");
    assert_eq!(press(&mut ed, "C-x C-s"), Outcome::Save("ba\n".into()));
    assert!(ed.is_dirty(), "nothing is saved until the tool says so");
    ed.saved();
    assert!(!ed.is_dirty());
    assert_eq!(ed.echo().as_deref(), Some("Saved"));
    assert_eq!(press(&mut ed, "C-x k"), Outcome::Cancel);
}

#[test]
fn the_mark_moves_with_an_edit_before_it() {
    let mut ed = emacs("hello world\n");
    press(&mut ed, "C-f C-f C-f C-f C-f C-SPC C-a");
    typed(&mut ed, "XY");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "XY world\n");
    press(&mut ed, "C-e C-y");
    assert_eq!(ed.text(), "XY worldhello\n");
}

#[test]
fn a_deletion_before_the_mark_pulls_it_back() {
    let mut ed = emacs("abcdef\n");
    press(&mut ed, "C-f C-f C-f C-SPC C-g C-a C-d C-d");
    assert_eq!(ed.text(), "cdef\n");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 1));
}

#[test]
fn undo_after_the_mark_leaves_the_mark() {
    let mut ed = emacs("abcdefgh\n");
    press(&mut ed, "C-e");
    typed(&mut ed, "X");
    press(&mut ed, "C-a C-f C-f C-f C-f C-SPC C-b C-b C-/");
    assert_eq!(ed.text(), "abcdefgh\n");
    assert_eq!(ed.cursor(), (0, 8), "undo moves point to the edit");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "abcd\n", "the region is point (8) to mark (4)");
}

#[test]
fn undo_in_a_run_of_equal_characters_leaves_the_mark() {
    let mut ed = emacs("xaaa\n");
    press(&mut ed, "C-e");
    typed(&mut ed, "a");
    press(&mut ed, "C-a C-f C-f C-SPC C-a C-/");
    assert_eq!(ed.text(), "xaaa\n");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "xa\n", "the region is point (4) to mark (2)");
}

#[test]
fn redo_in_a_run_of_equal_characters_leaves_the_mark() {
    let mut ed = emacs("xaaa\n");
    press(&mut ed, "C-e");
    typed(&mut ed, "a");
    press(&mut ed, "C-/ C-a C-f C-f C-SPC C-g C-a C-M-/");
    assert_eq!(ed.text(), "xaaaa\n");
    press(&mut ed, "C-a C-w");
    assert_eq!(ed.text(), "aaa\n", "the region is point (0) to mark (2)");
}

#[test]
fn redo_after_the_mark_leaves_the_mark() {
    let mut ed = emacs("abcdefgh\n");
    press(&mut ed, "C-e");
    typed(&mut ed, "X");
    press(&mut ed, "C-/ C-a C-f C-f C-f C-f C-SPC C-g C-b C-b C-M-/");
    assert_eq!(ed.text(), "abcdefghX\n");
    press(&mut ed, "C-a C-f C-f C-w");
    assert_eq!(ed.text(), "abefghX\n");
}

#[test]
fn an_edit_that_ends_a_search_moves_the_mark_it_set() {
    let mut ed = emacs("abc abc xyz\n");
    press(&mut ed, "C-e C-b C-b C-b C-b C-r");
    typed(&mut ed, "abc");
    assert_eq!(ed.cursor(), (0, 4));
    press(&mut ed, "C-d");
    assert_eq!(ed.text(), "abc bc xyz\n");
    press(&mut ed, "C-w");
    assert_eq!(
        ed.text(),
        "abc  xyz\n",
        "the mark followed the deletion to 6"
    );
}

#[test]
fn a_yank_sets_the_mark_at_its_start() {
    let mut ed = emacs("ab cd\n");
    press(&mut ed, "M-d C-SPC C-g C-e C-y C-w");
    assert_eq!(ed.text(), " cd\n");
}

#[test]
fn a_backward_deletion_before_the_mark_pulls_it_back() {
    let mut ed = emacs("abcdef\n");
    press(&mut ed, "C-f C-f C-f C-SPC C-g DEL C-x C-x");
    assert_eq!(ed.cursor(), (0, 2));
}

#[test]
fn text_inserted_at_the_mark_leaves_it() {
    let mut ed = emacs("ab\n");
    press(&mut ed, "C-f C-SPC C-g");
    typed(&mut ed, "X");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 1));
}

#[test]
fn a_deletion_around_the_mark_with_text_after_it() {
    let mut ed = emacs("abcdef\nxy\n");
    press(&mut ed, "C-f C-f C-SPC C-g C-a C-k C-x C-x");
    assert_eq!(ed.cursor(), (0, 0));
}

#[test]
fn set_mark_twice_leaves_no_region_for_del() {
    let mut ed = emacs("abc\n");
    press(&mut ed, "C-SPC C-SPC C-e DEL");
    assert_eq!(ed.text(), "ab\n");
}

#[test]
fn a_region_kill_with_point_first_prepends_to_the_last_kill() {
    let mut ed = emacs("ab cd\n");
    press(&mut ed, "C-f C-f C-f C-SPC C-g C-a M-d C-w C-y");
    assert_eq!(ed.text(), " abcd\n");
}

#[test]
fn a_rebound_keyboard_quit_answers_the_discard_question() {
    let file = KeysFile::parse(
        "preset: emacs\neditor:\n  C-]: keyboard-quit\n",
        std::path::Path::new("keys.yml"),
    )
    .unwrap();
    let mut ed = Editor::new("a\n", file.keymap(&SECTION).unwrap());
    typed(&mut ed, "b");
    press(&mut ed, "C-x k C-]");
    assert_eq!(ed.echo().as_deref(), Some("Quit"));
    assert!(ed.is_dirty());
}

#[test]
fn a_prompt_closes_without_asking() {
    let mut ed = Editor::single_line("", Keymap::preset(&SECTION, Preset::Default));
    typed(&mut ed, "ab");
    assert_eq!(press(&mut ed, "ESC"), Outcome::Cancel);
}

#[test]
fn c_g_closes_an_emacs_prompt() {
    let mut ed = single("");
    typed(&mut ed, "ab");
    assert_eq!(press(&mut ed, "C-g"), Outcome::Cancel);
    let mut ed = single("");
    assert_eq!(press(&mut ed, "ESC ESC ESC"), Outcome::Cancel);
}

#[test]
fn search_does_nothing_in_a_prompt() {
    let mut ed = single("");
    press(&mut ed, "C-s");
    typed(&mut ed, "abc");
    assert_eq!(press(&mut ed, "RET"), Outcome::Finish("abc".into()));
}

#[test]
fn save_submits_a_prompt() {
    let mut ed = single("q");
    assert_eq!(press(&mut ed, "C-x C-s"), Outcome::Finish("q".into()));
}

#[test]
fn tab_before_an_indented_mark_moves_the_mark() {
    let mut ed = emacs("    foo\n");
    press(&mut ed, "M-f M-b C-SPC C-SPC C-a TAB C-w");
    assert_eq!(
        ed.text(),
        "    foo\n",
        "the four inserted spaces are the region"
    );
}

#[test]
fn typing_a_repeated_character_before_the_mark_moves_it() {
    let mut ed = emacs("xx\n");
    press(&mut ed, "C-e C-SPC C-SPC C-a");
    typed(&mut ed, "x");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "x\n");
}

#[test]
fn an_insertion_beside_a_repeated_character_does_not_overlap_the_span() {
    let mut ed = emacs("aa\n");
    press(&mut ed, "C-SPC C-g C-e");
    typed(&mut ed, "a");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 0));
}

#[test]
fn a_longer_replacement_ending_at_the_mark_pushes_it_on() {
    let mut ed = emacs("ß d\n");
    press(&mut ed, "C-f C-SPC C-g C-a M-u");
    assert_eq!(ed.text(), "SS d\n");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 2));
}

#[test]
fn a_replacement_does_not_slide_past_the_mark() {
    let mut ed = emacs("Sß d\n");
    press(&mut ed, "C-f C-SPC C-g C-a M-u");
    assert_eq!(ed.text(), "SSS d\n");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 1));
}

#[test]
fn a_backward_deletion_in_a_run_pulls_the_mark_back() {
    let mut ed = emacs("xxx\n");
    press(&mut ed, "C-f C-f C-SPC C-SPC DEL C-x C-x");
    assert_eq!(ed.text(), "xx\n");
    assert_eq!(ed.cursor(), (0, 1));
}

#[test]
fn ending_a_search_clears_a_message_set_during_it() {
    let mut ed = emacs("ab\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "b");
    ed.set_message("Save refused");
    press(&mut ed, "RET");
    assert_eq!(ed.echo(), None);
}

#[test]
fn a_case_change_over_the_mark_leaves_it() {
    let mut ed = emacs("hello\n");
    press(&mut ed, "C-f C-f C-SPC C-g C-a M-u C-x C-x");
    assert_eq!(ed.text(), "HELLO\n");
    assert_eq!(ed.cursor(), (0, 2));
}

#[test]
fn the_edit_span_never_overlaps_its_ends() {
    let a: Vec<char> = "aa".chars().collect();
    let b: Vec<char> = "aaa".chars().collect();
    assert_eq!(edit_span(&a, &b, 2), (2, 0, 1));
    assert_eq!(edit_span(&a, &b, 0), (0, 0, 1));
    assert_eq!(edit_span(&b, &a, 0), (0, 1, 0));
}

#[test]
fn an_offset_past_the_text_is_clamped_to_its_end() {
    let lines = vec!["ab".to_string(), "c".to_string()];
    assert_eq!(offset(&lines, (0, 9)), 2);
    assert_eq!(offset(&lines, (7, 9)), 4);
}

#[test]
fn deleting_a_repeated_character_at_the_mark_pulls_it_back() {
    let mut ed = emacs("xxx\n");
    press(&mut ed, "C-f C-SPC C-SPC C-a C-d C-x C-x");
    assert_eq!(ed.text(), "xx\n");
    assert_eq!(ed.cursor(), (0, 0));
}

#[test]
fn a_deletion_around_the_mark_moves_it_to_the_deletion() {
    let mut ed = emacs("abcdef\n");
    press(&mut ed, "C-f C-f C-SPC C-g C-a C-k");
    assert_eq!(ed.text(), "");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 0));
}

#[test]
fn set_mark_twice_ends_the_region_and_keeps_the_mark() {
    let mut ed = emacs("abc\n");
    press(&mut ed, "C-SPC C-SPC");
    assert_eq!(ed.echo().as_deref(), Some("Mark deactivated"));
    press(&mut ed, "C-e");
    typed(&mut ed, "!");
    assert_eq!(ed.text(), "abc!\n", "no region to replace");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "", "the mark still bounds a kill");
}

#[test]
fn killing_a_region_backward_prepends_to_the_kill() {
    let mut ed = emacs("ab cd\n");
    press(&mut ed, "C-e C-SPC M-b C-w M-DEL");
    assert_eq!(ed.text(), "");
    press(&mut ed, "C-y");
    assert_eq!(ed.text(), "ab cd\n");
}

#[test]
fn a_kill_that_removes_nothing_breaks_a_run_of_kills() {
    let mut ed = emacs("ab\n");
    press(&mut ed, "C-f C-k C-k M-DEL");
    assert_eq!(ed.text(), "");
    press(&mut ed, "C-y");
    assert_eq!(ed.text(), "a\n");
    press(&mut ed, "M-y");
    assert_eq!(ed.text(), "b\n");
}

#[test]
fn a_case_command_reaches_the_next_word_on_a_later_line() {
    let mut ed = emacs("x.\nword\n");
    press(&mut ed, "C-e M-u");
    assert_eq!(ed.text(), "x.\nWORD\n");
}

#[test]
fn the_discard_question_asks_again_on_another_key() {
    let mut ed = emacs("a\n");
    typed(&mut ed, "b");
    press(&mut ed, "C-x k");
    press(&mut ed, "q");
    assert_eq!(
        ed.echo().as_deref(),
        Some("Please answer y or n.  Discard changes? (y or n)")
    );
}

#[test]
fn an_unchanged_text_is_not_dirty_and_closes_without_asking() {
    let mut ed = emacs("body\n");
    assert!(!ed.is_dirty());
    assert_eq!(press(&mut ed, "C-x k"), Outcome::Cancel);
}

#[test]
fn closing_a_changed_text_asks_once() {
    let mut ed = emacs("body\n");
    typed(&mut ed, "x");
    assert_eq!(press(&mut ed, "C-x C-c"), Outcome::Continue);
    assert_eq!(ed.echo().as_deref(), Some("Discard changes? (y or n)"));
    assert_eq!(press(&mut ed, "n"), Outcome::Continue);
    assert_eq!(ed.echo(), None);
    assert_eq!(ed.text(), "xbody\n");
    press(&mut ed, "C-x C-c");
    assert_eq!(press(&mut ed, "C-g"), Outcome::Continue);
    assert_eq!(ed.echo().as_deref(), Some("Quit"));
    press(&mut ed, "C-x C-c");
    assert_eq!(press(&mut ed, "y"), Outcome::Cancel);
}

#[test]
fn the_echo_line_shows_a_pending_prefix() {
    let mut ed = emacs("");
    press(&mut ed, "C-x");
    assert_eq!(ed.echo().as_deref(), Some("C-x-"));
    press(&mut ed, "C-q");
    assert_eq!(ed.echo().as_deref(), Some("C-x C-q is undefined"));
}

#[test]
fn movement_follows_emacs() {
    let mut ed = emacs("one two\nthree\n");
    press(&mut ed, "M-f");
    assert_eq!(ed.cursor(), (0, 3));
    press(&mut ed, "M-f");
    assert_eq!(ed.cursor(), (0, 7));
    press(&mut ed, "M-f");
    assert_eq!(ed.cursor(), (1, 5), "forward-word crosses the line end");
    press(&mut ed, "M-b");
    assert_eq!(ed.cursor(), (1, 0));
    press(&mut ed, "M-b");
    assert_eq!(ed.cursor(), (0, 4));
    press(&mut ed, "C-e");
    assert_eq!(ed.cursor(), (0, 7));
    press(&mut ed, "C-a C-n C-f");
    assert_eq!(ed.cursor(), (1, 1));
    press(&mut ed, "M->");
    assert_eq!(ed.cursor(), (1, 5));
    press(&mut ed, "M-<");
    assert_eq!(ed.cursor(), (0, 0));
}

#[test]
fn esc_then_a_key_is_the_meta_key() {
    let mut ed = emacs("one two\n");
    press(&mut ed, "ESC f");
    assert_eq!(ed.cursor(), (0, 3));
}

#[test]
fn kill_line_twice_kills_the_newline_and_yanks_as_one() {
    let mut ed = emacs("first\nsecond\n");
    press(&mut ed, "C-k C-k");
    assert_eq!(ed.text(), "second\n");
    press(&mut ed, "C-y");
    assert_eq!(ed.text(), "first\nsecond\n");
    assert_eq!(ed.cursor(), (1, 0), "the yank held the newline");
}

#[test]
fn a_kill_after_motion_starts_a_new_entry_and_yank_pop_steps_back() {
    let mut ed = emacs("alpha beta\n");
    press(&mut ed, "M-d");
    press(&mut ed, "C-e");
    press(&mut ed, "M-DEL");
    assert_eq!(ed.text(), " \n");
    press(&mut ed, "C-a C-y");
    assert_eq!(ed.text(), "beta \n");
    press(&mut ed, "M-y");
    assert_eq!(
        ed.text(),
        "alpha \n",
        "yank-pop replaces only the yanked text"
    );
    press(&mut ed, "M-y");
    assert_eq!(ed.text(), "beta \n", "the ring wraps");
}

#[test]
fn yank_pop_after_another_command_is_refused() {
    let mut ed = emacs("word\n");
    press(&mut ed, "M-d C-y C-f M-y");
    assert_eq!(
        ed.echo().as_deref(),
        Some("Previous command was not a yank")
    );
}

#[test]
fn backward_kills_prepend() {
    let mut ed = emacs("one two\n");
    press(&mut ed, "C-e M-DEL M-DEL");
    assert_eq!(ed.text(), "");
    press(&mut ed, "C-y");
    assert_eq!(ed.text(), "one two\n");
}

#[test]
fn the_mark_survives_motion_and_c_w_kills_the_region() {
    let mut ed = emacs("keep cut keep\n");
    press(&mut ed, "M-f C-f C-SPC M-f");
    assert_eq!(ed.echo(), None, "the next key clears 'Mark set'");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "keep  keep\n");
    press(&mut ed, "C-e C-y");
    assert_eq!(ed.text(), "keep  keepcut\n");
}

#[test]
fn c_w_kills_to_an_inactive_mark() {
    let mut ed = emacs("abc def\n");
    press(&mut ed, "C-SPC C-g M-f");
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), " def\n");
}

#[test]
fn c_w_without_a_mark_says_so() {
    let mut ed = emacs("abc\n");
    press(&mut ed, "C-w");
    assert_eq!(
        ed.echo().as_deref(),
        Some("The mark is not set now, so there is no region")
    );
}

#[test]
fn copy_region_keeps_the_text_and_yanks_it() {
    let mut ed = emacs("ab\n");
    press(&mut ed, "C-SPC C-e M-w C-y");
    assert_eq!(ed.text(), "abab\n");
}

#[test]
fn exchange_point_and_mark_swaps_them() {
    let mut ed = emacs("abcdef\n");
    press(&mut ed, "C-f C-SPC C-f C-f C-f");
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 1));
    press(&mut ed, "C-x C-x");
    assert_eq!(ed.cursor(), (0, 4));
    press(&mut ed, "C-w");
    assert_eq!(ed.text(), "aef\n");
}

#[test]
fn mark_whole_buffer_selects_everything() {
    let mut ed = emacs("a\nb\n");
    press(&mut ed, "C-x h C-w");
    assert_eq!(ed.text(), "");
}

#[test]
fn typing_with_a_region_inserts_and_ends_the_region() {
    let mut ed = emacs("abc\n");
    press(&mut ed, "C-SPC C-e");
    typed(&mut ed, "!");
    assert_eq!(ed.text(), "abc!\n");
}

#[test]
fn transpose_and_case_commands() {
    let mut ed = emacs("ab hello world\n");
    press(&mut ed, "C-f C-t");
    assert_eq!(ed.text(), "ba hello world\n");
    press(&mut ed, "M-u");
    assert_eq!(ed.text(), "ba HELLO world\n");
    press(&mut ed, "M-c");
    assert_eq!(ed.text(), "ba HELLO World\n");
    press(&mut ed, "M-b M-l");
    assert_eq!(ed.text(), "ba HELLO world\n");
    press(&mut ed, "C-e C-t");
    assert_eq!(
        ed.text(),
        "ba HELLO wordl\n",
        "at the end of a line, the two before point"
    );
}

#[test]
fn undo_reverses_an_edit() {
    let mut ed = emacs("abc\n");
    press(&mut ed, "C-k");
    assert_eq!(ed.text(), "");
    press(&mut ed, "C-/");
    assert_eq!(ed.text(), "abc\n");
    press(&mut ed, "C-x u");
    press(&mut ed, "C-k C-_");
    assert_eq!(ed.text(), "abc\n", "C-_ is C-/");
}

#[test]
fn open_line_leaves_point_before_the_new_line() {
    let mut ed = emacs("ab\n");
    press(&mut ed, "C-f C-o");
    assert_eq!(ed.text(), "a\nb\n");
    assert_eq!(ed.cursor(), (0, 1));
}

#[test]
fn isearch_forward_lands_after_the_match_and_repeats() {
    let mut ed = emacs("cat dog cat\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "cat");
    assert_eq!(ed.cursor(), (0, 3));
    assert_eq!(ed.echo().as_deref(), Some("I-search: cat"));
    press(&mut ed, "C-s");
    assert_eq!(ed.cursor(), (0, 11));
    press(&mut ed, "RET");
    assert_eq!(ed.echo(), None);
    assert_eq!(ed.cursor(), (0, 11));
}

#[test]
fn isearch_c_g_returns_to_the_start() {
    let mut ed = emacs("one two\n");
    press(&mut ed, "C-f C-s");
    typed(&mut ed, "two");
    assert_eq!(ed.cursor(), (0, 7));
    press(&mut ed, "C-g");
    assert_eq!(ed.cursor(), (0, 1));
    assert_eq!(ed.echo().as_deref(), Some("Quit"));
}

#[test]
fn isearch_is_literal_and_folds_case_for_a_lower_case_query() {
    let mut ed = emacs("a.b A.B\n");
    press(&mut ed, "C-e C-r");
    typed(&mut ed, "a.b");
    assert_eq!(
        ed.cursor(),
        (0, 4),
        "backward search lands on the match start"
    );
    assert_eq!(ed.echo().as_deref(), Some("I-search backward: a.b"));
    press(&mut ed, "C-g C-e C-r");
    typed(&mut ed, "a.B");
    assert_eq!(ed.echo().as_deref(), Some("Failing I-search backward: a.B"));
}

#[test]
fn isearch_del_shrinks_the_query() {
    let mut ed = emacs("ab ax\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "ax");
    assert_eq!(ed.cursor(), (0, 5));
    press(&mut ed, "DEL");
    assert_eq!(ed.echo().as_deref(), Some("I-search: a"));
    assert_eq!(ed.cursor(), (0, 1));
}

#[test]
fn another_command_ends_the_search_and_runs() {
    let mut ed = emacs("xy\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "y");
    press(&mut ed, "C-a");
    assert_eq!(ed.cursor(), (0, 0));
    assert_eq!(ed.echo(), None);
}

#[test]
fn c_s_twice_reuses_the_last_search() {
    let mut ed = emacs("q q q\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "q");
    press(&mut ed, "RET C-s C-s");
    assert_eq!(ed.cursor(), (0, 3));
}

#[test]
fn the_default_preset_is_the_widget_table_plus_save_and_close() {
    let mut ed = default("abc\n");
    press(&mut ed, "C-e C-u");
    typed(&mut ed, "d");
    assert_eq!(press(&mut ed, "C-s"), Outcome::Save("abcd\n".into()));
    press(&mut ed, "C-u");
    assert_eq!(ed.text(), "abc\n");
    press(&mut ed, "C-r");
    assert_eq!(ed.text(), "abcd\n");
    assert_eq!(
        press(&mut ed, "ESC"),
        Outcome::Continue,
        "dirty: asks first"
    );
    assert_eq!(press(&mut ed, "y"), Outcome::Cancel);
}

#[test]
fn the_default_preset_copies_and_cuts_a_region() {
    let mut ed = default("abc\n");
    press(&mut ed, "C-SPC C-e C-c C-y");
    assert_eq!(ed.text(), "abcabc\n");
    press(&mut ed, "C-a C-SPC C-f C-x");
    assert_eq!(ed.text(), "bcabc\n");
}

#[test]
fn a_single_line_editor_saves_on_ret_and_flattens_a_yank() {
    let mut ed = Editor::single_line("", Keymap::preset(&SECTION, Preset::Emacs));
    ed.ring.kill("a\nb".into(), Direction::Forward, false);
    press(&mut ed, "C-y");
    assert_eq!(press(&mut ed, "RET"), Outcome::Finish("a b".into()));
}

#[test]
fn an_unbound_control_key_is_undefined_not_inserted() {
    let mut ed = emacs("");
    press(&mut ed, "C-]");
    assert_eq!(ed.text(), "");
    assert_eq!(ed.echo().as_deref(), Some("C-] is undefined"));
}

#[test]
fn a_users_editor_section_rebinds_a_key() {
    let file = KeysFile::parse(
        "preset: emacs\neditor:\n  C-c C-s: save-buffer\n  C-x C-s: unbound\n",
        std::path::Path::new("keys.yml"),
    )
    .unwrap();
    let mut ed = Editor::new("a\n", file.keymap(&SECTION).unwrap());
    assert_eq!(press(&mut ed, "C-x C-s"), Outcome::Continue);
    assert_eq!(press(&mut ed, "C-c C-s"), Outcome::Save("a\n".into()));
}

#[test]
fn every_command_has_a_key_in_each_preset() {
    for preset in [Preset::Default, Preset::Emacs] {
        let map = Keymap::preset(&SECTION, preset);
        for &action in EditAction::ALL {
            // Emacs binds backward-kill-line to no key (it is C-0 C-k).
            let allowed_missing = (preset == Preset::Emacs && action == E::BackwardKillLine)
                || preset == Preset::Default
                    && matches!(
                        action,
                        E::Finish
                            | E::OpenLine
                            | E::TransposeChars
                            | E::UpcaseWord
                            | E::DowncaseWord
                            | E::CapitalizeWord
                            | E::YankPop
                            | E::ExchangePointAndMark
                            | E::MarkWholeBuffer
                            | E::IsearchForward
                            | E::IsearchBackward
                    );
            assert!(
                allowed_missing || !map.keys_for(action).is_empty(),
                "{preset:?} binds nothing to {}",
                action.name()
            );
        }
    }
}

#[test]
fn snapshot_text_and_echo_line() {
    let mut ed = emacs("# Title\n\nBody text.\n");
    press(&mut ed, "C-x");
    insta::assert_snapshot!(screen(&ed));
}

#[test]
fn snapshot_isearch() {
    let mut ed = emacs("# Title\n\nBody text.\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "body");
    insta::assert_snapshot!(screen(&ed));
}

#[test]
fn snapshot_discard_question() {
    let mut ed = emacs("# Title\n");
    typed(&mut ed, "x");
    press(&mut ed, "C-x k");
    insta::assert_snapshot!(screen(&ed));
}

fn single(text: &str) -> Editor {
    Editor::single_line(text, Keymap::preset(&SECTION, Preset::Emacs))
}

#[test]
fn open_line_does_nothing_in_a_prompt() {
    let mut ed = single("ab");
    press(&mut ed, "C-f C-o");
    assert_eq!(press(&mut ed, "RET"), Outcome::Finish("ab".into()));
}

#[test]
fn a_prompt_draws_no_echo_row() {
    let mut ed = single("query");
    press(&mut ed, "C-x");
    assert_eq!(ed.echo().as_deref(), Some("C-x-"));
    let mut terminal = Terminal::new(TestBackend::new(12, 2)).unwrap();
    terminal.draw(|f| f.render_widget(&ed, f.area())).unwrap();
    let drawn = terminal.backend().to_string();
    assert!(!drawn.contains("C-x-"), "{drawn}");
    assert!(drawn.contains("query"), "{drawn}");
}

#[test]
fn an_empty_search_keeps_the_last_search() {
    let mut ed = emacs("q q q\n");
    press(&mut ed, "C-s");
    typed(&mut ed, "q");
    press(&mut ed, "RET C-s RET C-s C-s");
    assert_eq!(ed.cursor(), (0, 3));
}
