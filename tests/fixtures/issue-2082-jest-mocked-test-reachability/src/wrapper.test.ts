import { renderWrapper } from "./wrapper";

jest.mock("./dependency", () => ({
  renderDependency: jest.fn(() => "mocked dependency"),
}));

describe("renderWrapper", () => {
  it("uses the replacement", () => {
    expect(renderWrapper()).toBe("mocked dependency");
  });
});
