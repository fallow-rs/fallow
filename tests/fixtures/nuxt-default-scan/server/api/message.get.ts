import lower from '#shared/formatters/lower'

export default defineEventHandler(() => ({
  message: lower('LOUD'),
}))
