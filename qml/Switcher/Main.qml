import QtQuick
import Quickshell
import Nyx.Shell

// Nyx Switcher in the shared Quickshell runtime. Its backend (nyx-switcher-backend)
// keeps the shortcuts' socket and the switching session; this draws the switcher card
// and the overview. The layer namespaces are the GTK switcher's.
Scope {
    id: root

    readonly property string pinnedScreen: Quickshell.env("NYX_SHELL_SCREEN") || ""

    BackendClient {
        id: sw
        app: "nyx-switcher"
        topics: ["switcher", "overview", "thumbs"]
    }

    SwitcherWindow {
        client: sw
        pinnedScreen: root.pinnedScreen
    }
    OverviewWindow {
        client: sw
        pinnedScreen: root.pinnedScreen
    }
}
