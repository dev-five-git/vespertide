import { ThemeScript } from '@devup-ui/react'
import { describe, expect, it } from 'bun:test'
import { render } from 'bun-test-env-dom'

import HomePage from '@/app/page'
import { HeaderProvider } from '@/components/header/header-provider'
import { SearchProvider } from '@/components/search/provider'
import { SheetRoute, SheetRouter } from '@/components/sheet/router'

describe('HomePage', () => {
  it('should render', () => {
    const { container } = render(<ThemeScript />)
    expect(container).toMatchSnapshot()
    expect(
      <SearchProvider>
        <SheetRouter>
          <SheetRoute name="mobile-menu">
            <SheetRoute name="search">
              <HeaderProvider>
                <HomePage />
              </HeaderProvider>
            </SheetRoute>
          </SheetRoute>
        </SheetRouter>
      </SearchProvider>,
    ).toMatchSnapshot()
  })
})
