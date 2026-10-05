# Claude provider strings — English (source/fallback).

claude-display-name = claude
claude-description = Claude Code as a list: pick a folder, press : for its sessions, and read every answer, tool call and result row by row. Runs the claude command you already have, with its own login.
claude-radio-permission-mode = permission mode
claude-setting-model = model override
claude-setting-extra-args = extra CLI args
claude-checkbox-stream-partial = stream responses token-by-token
claude-setting-folder = Claude Code's folder (its sessions and skills)

# Command names shown wherever the provider's commands are listed. `session`
# swaps the folder listing for the claude session, `browse` swaps back.
claude-command-session = session
claude-command-browse = folders
claude-command-skills = skills
claude-command-sessions = sessions
claude-command-delete-session = delete session

# The session list: a button to start one, the row that button opens for the
# prompt, and the label for a session Claude has not titled yet.
claude-new-session = new session
claude-prompt-label = Prompt:
claude-untitled-session = untitled session

# Deleting a session. The confirmation names the session after the question, so
# the row being removed cannot be misread off a neighbouring line.
claude-confirm-delete = delete this session permanently?
claude-confirm-delete-no = no, keep it
claude-confirm-delete-yes = yes, delete it
claude-session-deleted = session deleted:
claude-session-delete-failed = could not delete that session

# The tutorial's paragraphs about this program, under its programs section:
# <name>-tutorial, then <name>-tutorial-2 and so on, read until one is missing.
claude-tutorial = Claude, from the Store: press : to turn the list into a Claude session running in the folder you are in, with an input line at the bottom. The header then says first command mode, because a second : is available here. The folder matters, because it decides which project Claude can see, so walk into one with Right first if you want it working a level deeper. Type a prompt and press Enter, and the answer arrives as a tree of messages, tool calls, and results you can walk through. Escape returns to the folders. Pressing : again at the input line opens the project's skills in second command mode, where Enter drops the chosen skill into your prompt. Pressing : in a different folder starts a fresh session there, which replaces the previous conversation.
