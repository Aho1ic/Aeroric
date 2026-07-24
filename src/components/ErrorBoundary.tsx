import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { staticT } from "../i18n";
import s from "../styles";

interface Props {
  children: ReactNode;
  /** 用于在错误信息中标识面板，例如 "文件浏览器" */
  label?: string;
  /** 捕获到错误时的自定义回退 UI；不传则使用内置样式 */
  fallback?: (error: Error, reset: () => void) => ReactNode;
  onError?: (error: Error, info: ErrorInfo) => void;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(
      `[ErrorBoundary${this.props.label ? ` – ${this.props.label}` : ""}]`,
      error,
      info.componentStack,
    );
    this.props.onError?.(error, info);
  }

  reset = () => {
    this.setState({ error: null });
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    if (this.props.fallback) {
      return this.props.fallback(error, this.reset);
    }

    const label = this.props.label ?? staticT("errorBoundary.unknownError");

    return (
      <div style={s.errorBoundaryWrap}>
        <div style={s.errorBoundaryIcon}>⚠</div>
        <div style={s.errorBoundaryTitle}>
          {staticT("errorBoundary.panelRenderError", { label })}
        </div>
        <div style={s.errorBoundaryMessage}>{error.message || staticT("errorBoundary.unknownError")}</div>
        <button onClick={this.reset} style={s.errorBoundaryBtn}>
          {staticT("common.retry")}
        </button>
      </div>
    );
  }
}
