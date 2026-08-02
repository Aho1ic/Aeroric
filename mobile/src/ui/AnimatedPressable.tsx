import {
  Animated,
  Pressable,
  type PressableProps,
  type PressableStateCallbackType,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import { useRef, useState } from "react";

type AnimatedPressableStyle =
  | StyleProp<ViewStyle>
  | ((state: PressableStateCallbackType) => StyleProp<ViewStyle>);

// 普通 Pressable 会把 Animated.Value 当作 JS 样式对象解析,Expo Go 随即报
// "Transform with key of scale must be a number"。必须把 Pressable 包装成真正
// 的 Animated 组件,让 Animated.Value 交给 Animated 样式节点处理。
const AnimatedPressableBase = Animated.createAnimatedComponent(Pressable);

/** 统一手机端按压反馈:轻微回弹缩放,不引入额外动画依赖。 */
export function AnimatedPressable({
  children,
  style,
  onPressIn,
  onPressOut,
  ...props
}: Omit<PressableProps, "style"> & { style?: AnimatedPressableStyle }) {
  const scale = useRef(new Animated.Value(1)).current;
  const [pressed, setPressed] = useState(false);

  const animateTo = (value: number) => {
    Animated.spring(scale, {
      toValue: value,
      speed: 28,
      bounciness: 3,
      useNativeDriver: true,
    }).start();
  };

  return (
    <AnimatedPressableBase
      {...props}
      onPressIn={(event) => {
        setPressed(true);
        animateTo(0.975);
        onPressIn?.(event);
      }}
      onPressOut={(event) => {
        setPressed(false);
        animateTo(1);
        onPressOut?.(event);
      }}
      style={[typeof style === "function" ? style({ pressed }) : style, { transform: [{ scale }] }]}
    >
      {children}
    </AnimatedPressableBase>
  );
}
