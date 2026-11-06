export class Link {
  id = ''
  label = ''

  setId(id: string): this {
    this.id = id
    return this
  }

  setLabel(label: string): this {
    this.label = label
    return this
  }

  reset(): this {
    return this
  }

  unusedChainable(): this {
    return this
  }

  configure(id: string, label: string): this {
    return this.setId(id).reset().setLabel(label)
  }
}
