import { Component, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";

interface Props {
  children: ReactNode;
  fallbackMessage?: string;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export default class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  render() {
    if (this.state.hasError) {
      const isTauriError = this.state.error?.message?.includes("invoke") ||
        this.state.error?.message?.includes("__TAURI__");
      return (
        <div className="flex flex-col items-center justify-center h-full p-8 text-center">
          <AlertTriangle size={40} className="text-yellow-500 mb-4" />
          <h2 className="text-lg font-semibold mb-2">
            {isTauriError ? "Desktop app required" : "Something went wrong"}
          </h2>
          <p className="text-sm text-cs-muted max-w-md mb-4">
            {isTauriError
              ? "This feature requires the desktop app (Tauri). Run 'npm run dev:desktop' instead of 'npm run dev' to use all features."
              : this.props.fallbackMessage ?? this.state.error?.message ?? "An unexpected error occurred."}
          </p>
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            className="px-4 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:bg-cs-accent/90"
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
