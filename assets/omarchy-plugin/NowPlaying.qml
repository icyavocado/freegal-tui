import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui

BarWidget {
  id: root
  moduleName: "freegal.now-playing"

  readonly property string stateDir: {
    const s = Quickshell.env("XDG_STATE_HOME");
    return (s && s.length > 0 ? s : Quickshell.env("HOME") + "/.local/state");
  }
  readonly property string statePath: stateDir + "/freegal/bar.json"
  readonly property int maxLabelWidth: Number(setting("maxWidth", 280))

  property int refresh: 0
  readonly property var state: _read()
  readonly property string title: state ? (state.title || "") : ""
  readonly property string artist: state ? (state.artist || "") : ""
  readonly property bool playing: state ? (state.playing === true) : false

  function _read() {
    const touch = root.refresh;
    try {
      return JSON.parse(watcher.text());
    } catch (e) {
      return null;
    }
  }

  FileView {
    id: watcher
    path: root.statePath
    watchChanges: true
    onFileChanged: root.refresh += 1
    onLoaded: root.refresh += 1
    onLoadFailed: root.refresh += 1
  }

  visible: title !== "" && !vertical
  implicitWidth: visible ? Math.min(maxLabelWidth, label.implicitWidth) + Style.spacing.controlPaddingX * 2 : 0
  implicitHeight: barSize

  Behavior on implicitWidth {
    NumberAnimation { duration: 180; easing.type: Easing.OutCubic }
  }

  Item {
    anchors.fill: parent
    anchors.leftMargin: Style.space(8)
    anchors.rightMargin: Style.space(8)
    clip: true

    Text {
      id: label
      textFormat: Text.PlainText
      anchors.verticalCenter: parent.verticalCenter
      anchors.left: parent.left
      width: parent.width
      text: (root.playing ? "♪ " : "⏸ ") + root.title + (root.artist !== "" ? " — " + root.artist : "")
      color: root.playing ? (root.bar ? root.bar.barAccent : Commons.Color.accent) : (root.bar ? root.bar.barForeground : Commons.Color.foreground)
      font.family: root.bar ? root.bar.fontFamily : Style.font.family
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
      opacity: 0.9
    }
  }

  Process {
    id: launcher
    command: ["sh", "-c", root.setting("launchCmd", "xdg-terminal-exec freegal")]
  }

  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    acceptedButtons: Qt.LeftButton
    cursorShape: Qt.PointingHandCursor

    onClicked: function(mouse) {
      launcher.running = true;
    }
    onEntered: if (root.bar) root.bar.showTooltip(root, "open freegal")
    onExited: if (root.bar) root.bar.hideTooltip(root)
  }
}