import libxmljs from "libxmljs";

export function parse(xml: string): unknown {
  return libxmljs.parseXml(xml, { noent: true });
}
