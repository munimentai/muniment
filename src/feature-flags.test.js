import { expect, it } from 'vitest'
import { readFeatureFlags } from './feature-flags.js'
it('ships with both features off and requires an explicit build flag for each', () => {
  expect(readFeatureFlags()).toEqual({cloud: false, companyRecord: false})
  expect(readFeatureFlags({VITE_MUNIMENT_CLOUD: 'true'})).toEqual({cloud: true, companyRecord: false})
  expect(readFeatureFlags({VITE_MUNIMENT_COMPANY_RECORD: 'true'})).toEqual({cloud: false, companyRecord: true})
  for (const value of ['false', '0', '1', '', 'TRUE', true, undefined]) {
    expect(readFeatureFlags({VITE_MUNIMENT_CLOUD: value, VITE_MUNIMENT_COMPANY_RECORD: value})).toEqual({cloud: false, companyRecord: false})
  }
})
