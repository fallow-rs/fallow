import { useEffect, useRef } from "react";
import { WebView } from "react-native-webview";

interface CheckoutMessage {
  script: string;
}

export const CheckoutView = ({ message }: { message: CheckoutMessage }) => {
  const webViewRef = useRef<WebView>(null);

  useEffect(() => {
    webViewRef.current?.injectJavaScript(message.script);
  }, [message]);

  return <WebView ref={webViewRef} source={{ uri: "https://shop.example.com/checkout" }} />;
};
