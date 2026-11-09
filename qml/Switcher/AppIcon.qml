import QtQuick
import Quickshell

// An app's icon: the first of `names` the icon theme has (the app's own, then guesses
// from its window class), else a generic one.
Image {
    id: i

    property var names: []
    property int size: 56
    readonly property string found: {
        for (const n of names || []) {
            if (!n)
                continue;
            if (n.startsWith("/"))
                return "file://" + n;
            const p = Quickshell.iconPath(n, true);
            if (p)
                return p;
        }
        return Quickshell.iconPath("application-x-executable", true);
    }

    width: size
    height: size
    source: found
    sourceSize: Qt.size(size * 2, size * 2)
    fillMode: Image.PreserveAspectFit
    asynchronous: true
    smooth: true
    mipmap: true
}
