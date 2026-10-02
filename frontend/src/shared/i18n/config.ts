import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import { sdlcLocales } from '@sdlc/ui/i18n'
import ru from './locales/ru.json'

i18n.use(initReactI18next).init({
  // Fleet-shared translations (services-base @sdlc/ui/i18n) fill gaps;
  // local keys win on conflicts.
  defaultNS: 'ci-cd',
  fallbackNS: 'base',
  resources: {
    en: { base: sdlcLocales.en, "ci-cd": en },
    ru: { base: sdlcLocales.ru, "ci-cd": ru },
  },
  lng: 'ru',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
})

export default i18n
