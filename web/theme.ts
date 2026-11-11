import { definePreset } from '@primeuix/themes'
import Aura from '@primeuix/themes/aura'
export default {
  preset: definePreset(Aura, {
    semantic: {
      primary: { 50: '#faf5ff', 100: '#f0e2f8', 200: '#dfc2ee', 300: '#c493df', 400: '#a35fc7', 500: '#7b32a4', 600: '#5e1883', 700: '#410962', 800: '#35064f', 900: '#29043e', 950: '#190126' },
      colorScheme: {
        light: { primary: { color: '#410962', inverseColor: '#ffffff', hoverColor: '#35064f', activeColor: '#29043e' } },
        dark: { primary: { color: '#c493df', inverseColor: '#190126', hoverColor: '#dfc2ee', activeColor: '#f0e2f8' } },
      },
    },
  }),
  options: { darkModeSelector: 'system' },
}
