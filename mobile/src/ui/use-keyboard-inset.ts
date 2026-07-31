/**
 * 键盘底部占位高度(window 坐标系):iOS 用 willChangeFrame 平滑跟随,
 * Android 依赖 softwareKeyboardLayoutMode: "resize" 由系统缩窗,返回 0。
 */

import { useEffect, useState } from "react";
import { Dimensions, Keyboard, Platform } from "react-native";

export function useKeyboardInset(): number {
  const [inset, setInset] = useState(0);

  useEffect(() => {
    if (Platform.OS !== "ios") return;
    const subs = [
      Keyboard.addListener("keyboardWillChangeFrame", (event) => {
        const windowHeight = Dimensions.get("window").height;
        setInset(Math.max(0, windowHeight - event.endCoordinates.screenY));
      }),
      Keyboard.addListener("keyboardWillHide", () => setInset(0)),
    ];
    return () => subs.forEach((sub) => sub.remove());
  }, []);

  return inset;
}
