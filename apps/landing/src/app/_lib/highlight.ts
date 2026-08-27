import { codeToHtml } from 'shiki'

export type CodeLang = 'json' | 'shell' | 'rust'

const LANG_MAP: Record<CodeLang, string> = {
  json: 'json',
  shell: 'bash',
  rust: 'rust',
}

export async function highlight(code: string, lang: CodeLang): Promise<string> {
  return codeToHtml(code, {
    lang: LANG_MAP[lang],
    themes: {
      light: 'github-light',
      dark: 'github-dark',
    },
  })
}
