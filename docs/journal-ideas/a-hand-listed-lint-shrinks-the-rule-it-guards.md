# A hand-listed lint shrinks the rule it guards

Our desktop client keeps its design system in a short standard file. One line of that standard says every control presents a hit area of at least 24 by 24 CSS pixels. That rule is worth a merge gate, because a control three pixels short still looks correct in a screenshot.

The original enforcement used a source-reading lint rather than a browser. The lint opened the main application component, split its style block into rules, and checked four named selectors. Each had to declare a minimum width and a minimum height of at least 24 pixels. The lint carried two negative tests. One shrank a value to 23 pixels and asserted the lint reported it. The other deleted a minimum width and asserted the same. The lint was correct, and it stayed green during its first four days.

We then measured a second surface by hand. Our profile panel is a separate component. It holds the controls for appearance, account access, paired devices, connected programs, and a dictation shortcut. Two of its eight controls miss the floor. The control that revokes a paired program's access renders 56 by 21 pixels. The control that closes the panel renders 21 by 28 pixels. Both are three pixels short. Neither rule declares a minimum width or a minimum height. The comparable control in the main component, a row action that deletes a conversation, declares both and measures 56 by 24.

The original lint never saw either one. It opened one file and checked a list of four selector names that a person typed. The rule was repository wide. The enforcement was one file wide and four selectors deep. Nothing in the merge gate noticed that difference, and the lint's own failure output gave no hint that its list was partial.

The revoke control is the sharper case. It is the only way to withdraw a paired program's access. A security decision should not sit behind an undersized target.

Here is what this does not show. It does not prove that 24 pixels is the right floor, and it measures no real-world miss rate. We have not counted every control in the repository, so we cannot say what fraction of them the original four-selector list covered. After this measurement, the repository widened the lint to read two files and check six selectors. The added selectors cover `.companion-revoke` and `.close-access`. What we can report is the arithmetic. A design rule applied to every control. The original lint read one of many component files. Two violations stayed green.

The transferable part is a question to ask whenever a lint encodes a rule. State the rule's scope and the lint's scope in the same sentence. When the rule said every control and the original lint said four selectors in one file, the difference was the blind spot. Writing it down costs a minute. Finding it later costs a measurement session.
