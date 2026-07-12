export const SCROLL_FOLLOW_THRESHOLD = 48

export function scrollFollowState({
  pinned,
  scrollTop,
  scrollHeight,
  clientHeight,
  lastScrollTop,
  threshold = SCROLL_FOLLOW_THRESHOLD,
}) {
  const atBottom = scrollHeight - clientHeight - scrollTop <= threshold
  const scrolledUp = scrollTop < lastScrollTop

  return {
    pinned: atBottom || (pinned && !scrolledUp),
    lastScrollTop: scrollTop,
  }
}
