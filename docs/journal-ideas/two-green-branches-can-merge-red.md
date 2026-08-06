# Two green branches can merge red

We run a desktop client whose pull requests each gate on a full test suite. Two changes went in on the same day, both green, and the merge of the two was broken. Neither branch had rebased on the other. Each pull request built against its own base, and no build ever ran the combination.

That first case cost a build. The second case cost a shipped feature. One change added a control to a settings surface, with tests that asserted the control renders and works. A second change, prepared from a base older than that merge, touched the same component. When it merged, it silently removed both the control and the tests that guarded it. Both pull requests were green at merge time. The later one was green precisely because it had deleted the failing evidence. A test that no longer exists cannot fail. A third change restored the control and its tests.

The general shape is worth naming. A merge gate that runs a branch's own tests answers one question: does this branch pass? It does not answer whether the result of merging that branch passes. Those are the same question only when the branch is up to date with the target. A deleted test makes the gap invisible rather than loud, because lost coverage produces no red.

We did not fix this in application code, and that is the honest part. The fix is a repository setting. One option requires a branch to be up to date before merge. The other puts a merge queue in front of the target branch. Both cost wall-clock time on every merge, and both are an owner decision rather than ours. So we hold a diagnosis and a mitigation we have not measured. This note does not prove that a merge queue would have caught either case, and it measures no cost.

Two habits helped in the meantime, and neither is a substitute. Every ticket that touches a hot file now tells the implementer to rebase before opening the pull request. And a review that removes a test asks the reviewer to say why out loud, so a silent deletion becomes a visible claim.
