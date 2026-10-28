# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

## AI Agents page (settings/src/pages/agent.rs). The lists follow the app lists in macOS Privacy & Security

agent-description = Choose whether AI can read and control the apps on this computer. Every change asks for an administrator password.
agent-clients-title = AI That Can Control This Computer
agent-clients-description = AI with this access can read what apps show, click buttons, type text and otherwise control this computer.
agent-apps-title = Apps AI Can’t Access
agent-apps-description = AI can’t read or control the apps listed here, even if it is allowed above.
agent-empty = No Items
agent-limit-note = These settings apply to access through the buntOS agent (bunt agent). Programs running as the same user can still connect to the accessibility bus directly.
# Names of the [+] and [−] buttons (they show only icons; used for tooltips and accessibility)
agent-add-client = Add AI
agent-remove-client = Remove AI
agent-add-app = Add App
agent-remove-app = Remove App
agent-cancel = Cancel
agent-add = Add
agent-client-sheet-title = Add AI That Can Control This Computer
agent-client-sheet-description = These AI programs asked for access recently. To add another program, choose “Other Program…”.
agent-client-sheet-empty = No AI has asked for access.
agent-other-program = Other Program…
agent-program-dialog-title = Choose an AI Program
agent-app-sheet-title = Add Apps AI Can’t Access
agent-app-sheet-description = AI won’t be able to read or control the apps you choose.
agent-app-search = Search
agent-app-sheet-empty = There are no more apps to add.
agent-not-authorized = Nothing was changed because authentication failed.
agent-change-failed = Couldn’t make the change: { $error }
agent-program-shell = Shells and shell scripts can’t be added.
agent-program-interpreter = Scripts for this interpreter aren’t supported yet.
agent-program-failed = Couldn’t read the program: { $error }
