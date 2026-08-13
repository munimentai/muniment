# A hand-listed lint shrinks the rule it guards

Our desktop client keeps its design system in a short standard file. One line of that standard says every control presents a hit area of at least 24 by 24 CSS pixels. That rule is worth a merge gate, because a control three pixels short still looks correct in a screenshot.

We enforce it with a source-reading lint rather than a browser. The lint opens the main application component, splits its style block into rules, and checks four named selectors. Each must declare a minimum width and a minimum height of at least 24 pixels. The lint carries two negative tests. One shrinks a value to 23 pixels and asserts the lint reports it. The other deletes a minimum width and asserts the same. The lint is correct, and it stayed green during its first four days.

We then measured a second surface by hand. Our profile panel is a separate component. It holds the controls for appearance, account access, paired devices, connected programs, and a dictation shortcut. Two of its eight controls miss the floor. The control that revokes a paired program's access renders 56 by 21 pixels. The control that closes the panel renders 21 by 28 pixels. Both are three pixels short. Neither rule declares a minimum width or a minimum height. The comparable control in the main component, a row action that deletes a conversation, declares both and measures 56 by 24.

The lint never saw either one. It opens one file and checks a list of four selector names that a person typed. The rule is repository wide. The enforcement is one file wide and four selectors deep. Nothing in the merge gate notices that difference, and the lint's own failure output gives no hint that its list is partial.

The revoke control is the sharper case. It is the only way to withdraw a paired program's access. A security decision should not sit behind an undersized target.

Here is what this does not show. It does not prove that 24 pixels is the right floor, and it measures no real-world miss rate. We have not counted every control in the repository, so we cannot say what fraction of them the four-selector list covers. We have not shipped the widened lint either, so we cannot report what else it finds. What we can report is the arithmetic. A design rule applies to every control. A lint reads one of many component files. Two violations stayed green.

The transferable part is a question to ask whenever a lint encodes a rule. State the rule's scope and the lint's scope in the same sentence. When the rule says every control and the lint says these four selectors in this one file, the difference is your blind spot. Writing it down costs a minute. Finding it later costs a measurement session.
