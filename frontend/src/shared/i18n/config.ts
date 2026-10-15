import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import { pdlcLocales } from '@pdlc/ui/i18n'
import ru from './locales/ru.json'

i18n.use(initReactI18next).init({
  // Fleet-shared translations (services-base @pdlc/ui/i18n) fill gaps;
  // local keys win on conflicts.
  defaultNS: 'ci-cd',
  fallbackNS: 'base',
  resources: {
    en: { base: pdlcLocales.en, "ci-cd": en },
    ru: { base: pdlcLocales.ru, "ci-cd": ru },
  },
  lng: 'ru',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
})

export default i18n
