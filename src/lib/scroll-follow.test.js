import { describe, expect, it } from 'vitest'

import { scrollFollowState } from './scroll-follow.js'

const viewport = {
  clientHeight: 400,
  threshold: 48,
}

describe('scrollFollowState', () => {
  it('stays pinned when content grows at the bottom', () => {
    expect(scrollFollowState({
      ...viewport,
      pinned: true,
      scrollTop: 600,
      lastScrollTop: 600,
      scrollHeight: 1100,
    }).pinned).toBe(true)
  })

  it('unpins on an upward user scroll', () => {
    expect(scrollFollowState({
      ...viewport,
      pinned: true,
      scrollTop: 520,
      lastScrollTop: 600,
      scrollHeight: 1000,
    }).pinned).toBe(false)
  })

  it('re-pins after a downward return within the threshold', () => {
    expect(scrollFollowState({
      ...viewport,
      pinned: false,
      scrollTop: 565,
      lastScrollTop: 500,
      scrollHeight: 1000,
    }).pinned).toBe(true)
  })

  it('does not re-pin after programmatic growth while unpinned', () => {
    expect(scrollFollowState({
      ...viewport,
      pinned: false,
      scrollTop: 300,
      lastScrollTop: 300,
      scrollHeight: 1200,
    }).pinned).toBe(false)
  })
})
