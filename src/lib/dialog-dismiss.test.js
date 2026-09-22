import { describe, it, expect, vi } from 'vitest'
import { dialogDismiss } from './dialog-dismiss.js'

describe('dialog dismissal', () => {
  function setup(disabled = false) {
    const node = document.createElement('dialog')
    node.getBoundingClientRect = () => ({left:100, right:300, top:100, bottom:250})
    const onclose = vi.fn()
    const action = dialogDismiss(node, {onclose, disabled})
    const pointer = (type, x, y) => node.dispatchEvent(new MouseEvent(type, {clientX:x, clientY:y, bubbles:true}))
    return {node, onclose, action, pointer}
  }
  it('closes for backdrop clicks but not panel whitespace or drags out of the panel', () => {
    const {onclose, pointer, action} = setup()
    pointer('pointerdown',150,150); pointer('click',150,150)
    pointer('pointerdown',150,150); pointer('click',50,50)
    expect(onclose).not.toHaveBeenCalled()
    pointer('pointerdown',50,50); pointer('click',50,50)
    expect(onclose).toHaveBeenCalledOnce()
    action.destroy()
  })
  it('blocks Escape and outside clicks while pending, and removes listeners on teardown', () => {
    const {node, onclose, pointer, action} = setup(true)
    const cancel = new Event('cancel', {cancelable:true})
    node.dispatchEvent(cancel)
    expect(cancel.defaultPrevented).toBe(true)
    pointer('pointerdown',50,50); pointer('click',50,50)
    expect(onclose).not.toHaveBeenCalled()
    action.update({onclose, disabled:false})
    node.dispatchEvent(new Event('cancel', {cancelable:true}))
    expect(onclose).toHaveBeenCalledOnce()
    action.destroy()
    node.dispatchEvent(new Event('cancel'))
    expect(onclose).toHaveBeenCalledOnce()
  })
})
