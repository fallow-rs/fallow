// Positive: react-native-webview injectJavaScript (member-call) and the
// injectedJavaScript prop (jsx-attr) run their argument as JS inside the embedded
// web context (CWE-94). Both rows are gated on the react-native-webview enabler.
import { WebView } from "react-native-webview";

interface WebViewRef {
  injectJavaScript(script: string): void;
}

export function runScript(ref: WebViewRef, script: string): void {
  ref.injectJavaScript(script);
}

export function WebViewWithScript(props: { script: string }): JSX.Element {
  return <WebView injectedJavaScript={props.script} />;
}
