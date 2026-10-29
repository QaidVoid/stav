import type { Theme } from 'vitepress'
import DefaultTheme from 'vitepress/theme'

import '@fontsource-variable/atkinson-hyperlegible-next/wght.css'
import '@fontsource-variable/atkinson-hyperlegible-next/wght-italic.css'
import '@fontsource-variable/bitter/wght.css'
import '@fontsource-variable/source-code-pro/wght.css'
import '@fontsource-variable/source-code-pro/wght-italic.css'
import './style.css'

import Home from './components/Home.vue'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('Home', Home)
  },
} satisfies Theme
