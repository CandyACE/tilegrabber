export default {
  appName: "御图",
  app: {
    networkOffline:
      "Network offline — downloads paused automatically and will resume when reconnected",
    diskFull:
      "Disk space low (< 200 MB) — task paused automatically. Free up space and resume manually.",
  },

  common: {
    cancel: "Cancel",
    confirm: "Confirm",
  },

  nav: {
    map: "Map",
    tasks: "Tasks",
    publish: "Publish",
    remote: "Remote",
    settings: "Settings",
    help: "Help",
    about: "About",
  },

  header: {
    collapseSidebar: "Collapse Sidebar",
    expandSidebar: "Expand Sidebar",
    apiRunning: "API Running",
    apiStopped: "API Stopped",
    minimize: "Minimize",
    maximize: "Maximize",
    restore: "Restore",
    close: "Close",
    updateAvailable: "Update Available",
  },

  splash: {
    subtitle: "Offline Map Tile Download & Manager",
  },

  disclaimer: {
    tag: "IMPORTANT NOTICE",
    title: "User Notice & Disclaimer",
    intro:
      "This software ({appName}) is intended solely for personal learning, research, and lawful offline map use cases.\nPlease read all the following terms carefully before continuing.",
    clause01Title: "Copyright Compliance",
    clause01Body:
      "Map data is copyrighted by the respective data providers. Ensure your use complies with their terms of service. Commercial redistribution or violation of data provider license agreements is prohibited.",
    clause02Title: "Lawful Use",
    clause02Body:
      "Any use that violates national laws, regulations, or infringes upon the rights of others is strictly prohibited. Classified map data is subject to confidentiality laws — unauthorized downloading or storage is forbidden.",
    clause03Title: "Use at Your Own Risk",
    clause03Body:
      "Any legal liability or losses arising from the use of this software are solely the responsibility of the user. The software developer bears no joint liability.",
    clause04Title: "Ban Risk",
    clause04Body:
      "Frequent or large-scale tile requests may trigger anti-scraping mechanisms of data providers, causing your IP or account to be rate-limited or banned. Use with caution.",
    disagree: "Disagree, Exit",
    agree: "I have read and agree →",
  },

  closeDialog: {
    title: "Close Window",
    subtitle: "Choose what happens when you close the window",
    optTrayLabel: "Minimize to System Tray",
    optTrayDesc:
      "The application continues running in the background and can be reopened from the tray icon",
    optQuitLabel: "Quit Application",
    optQuitDesc: "Close all tasks and exit the application",
    remember: "Remember my choice and do not ask again",
    cancel: "Cancel",
    confirm: "Confirm",
  },

  settings: {
    title: "Application Settings",
    save: "Save",
    saved: "Saved",
    saving: "Saving…",
    unsaved: "Unsaved",
    unsavedHint: "You have unsaved changes",
    leaveConfirm: "You have unsaved changes. Leave and discard them?",
    resetDefaults: "Reset Defaults",

    groups: {
      app: "Application",
      download: "Download",
      server: "Publish Server",
      network: "Network Proxy",
      rules: "Download Rules (Anti-ban)",
      update: "Auto Update",
      remote: "Remote Collaboration",
    },

    fields: {
      app_tiles_dir: {
        label: "Tile Storage Directory",
        hint: 'Location where tile files are saved. Leave empty to use the default "Documents/御图/tiles" folder (always writable).',
        placeholder: "Default: install dir/tiles",
      },
      app_float_window: {
        label: "Show Floating Speed Window",
        hint: "When the main window is minimized to tray, show a draggable floating speed indicator. Double-click to restore the main window.",
      },
      app_download_notification: {
        label: "System Notification on Download Completion",
        hint: "Send an OS notification when a task finishes downloading (or completes with errors).",
      },
      app_close_action: {
        label: "When clicking the Close button",
        hint: "Action when pressing the window close button (or Alt+F4)",
        optAsk: "Ask every time",
        optTray: "Minimize to system tray",
        optQuit: "Quit immediately",
      },
      app_language: {
        label: "Interface Language",
        hint: 'Click "Save" after changing the language for it to take effect',
        optAuto: "Follow System",
        optZh: "中文",
        optEn: "English",
      },
      download_concurrency: {
        label: "Concurrency",
        hint: "Number of simultaneous tile download threads ({cores}-core CPU, recommended {suggested}+). Too high may trigger server rate limiting.",
      },
      download_timeout_secs: {
        label: "Timeout (seconds)",
        hint: "Maximum wait time for a single tile request",
      },
      download_max_retries: {
        label: "Max Retries",
        hint: "Maximum number of automatic retries after failure",
      },
      download_retry_delay_ms: {
        label: "Retry Delay Base (ms)",
        hint: "Initial retry wait time, doubles with each subsequent retry",
      },
      download_delay_min_ms: {
        label: "Request Interval Min (ms)",
        hint: "Minimum random delay between adjacent tile requests",
      },
      download_delay_max_ms: {
        label: "Request Interval Max (ms)",
        hint: "Maximum random delay between adjacent tile requests",
      },
      tasks_max_concurrent: {
        label: "Max Concurrent Tasks",
        hint: "Maximum number of simultaneously running download tasks. 0 = unlimited.",
      },
      server_default_port: {
        label: "Default Port",
        hint: "Default listening port for the tile publish server",
      },
      server_bind_lan: {
        label: "Allow LAN Access",
        hint: "When disabled, only this computer can connect. A token is required when enabled.",
      },
      server_token: {
        label: "LAN Access Token",
        hint: "LAN clients must send this Bearer Token with requests",
      },
      remote_enabled: {
        label: "Enable Remote Collaboration API",
        hint: "Once enabled, other TileGrabber clients can connect using a Token to submit download tasks and monitor progress in real time",
      },
      remote_port: {
        label: "Remote Server Port",
        hint: "Port for the remote collaboration server — independent from the tile publish server",
      },
      remote_token: {
        label: "Access Token",
        hint: "Clients must supply this token to connect. Empty or not generated = all connections rejected",
      },
      network_proxy_enabled: {
        label: "Use Custom Proxy",
        hint: "Override the system proxy; when disabled, the operating system proxy is used automatically",
      },
      network_proxy_url: {
        label: "Proxy URL",
        hint: "Supports http, https, socks5 — e.g. http://127.0.0.1:7890",
        placeholder: "http://127.0.0.1:7890",
      },
    },

    update: {
      currentVersion: "Current version:",
      checking: "Checking…",
      checkAgain: "Check Again",
      checkNow: "Check for Updates",
      newVersion: "New version v{version} available",
      autoDetected: "Auto-detected in background",
      manualCheck: "Auto-detected",
      upToDate: "Already up to date v{version}",
      upToDateDesc: "You are running the latest version",
      downloadInstall: "Download & Install",
      goToDownload: "Go to Download Page",
      downloading: "Downloading… {percent}%",
      downloadDone: "Download complete, launching installer…",
      releaseNotes: "Release Notes",
      checkError: "Check failed: {error}",
      networkError: "Request failed: {error}",
      installError: "Install failed: {error}",
      selectFile: "Select Update Package",
    },

    remote: {
      generate: "Generate New Token",
      generating: "Generating…",
      copy: "Copy",
      copied: "Copied",
      tokenEmpty: 'No token yet — click "Generate" to create one',
      connectedClients: "Currently connected clients",
      noClients: "No clients connected",
      clientCount: "{count} client(s) connected",
      serverRunning: "Remote server is running",
      serverStopped: "Remote server is not running",
      serverStoppedHint:
        "Other clients cannot connect until the remote server is started",
      startServer: "Start Now",
      starting: "Starting…",
      lanAddress: "LAN address (share with others)",
      firewallHint: "Make sure your firewall allows inbound TCP on port {port}",
    },

    selectFolder: "Select Directory",

    storage: {
      title: "Storage Usage",
      diskUsage: "Disk Usage",
      available: "Available",
      appUsage: "This App",
      otherUsage: "Other Usage",
      openDir: "Open Folder",
      perTask: "Per-Task Usage (sorted by size)",
      showMore: "Show more ({count})",
      showLess: "Show less",
      refresh: "Refresh",
      orphansTitle: "{count} orphan tile file(s) found · {size}",
      orphansHint:
        "These .tiles files exist in the default directory but aren't referenced by any task — likely leftovers from deleted tasks.",
      cleanupAll: "Clean All",
      cleaning: "Cleaning…",
      confirmCleanup:
        "This will permanently delete {count} orphan tile file(s) ({size}). Continue?",
    },
  },

  remote: {
    title: "Remote Collaboration",
    modeServer: "Server Mode",
    modeClient: "Client Mode",
    addServer: "Add Server",
    serverList: "Saved Servers",
    noServers: 'No servers saved yet — click "Add Server" to get started',
    connect: "Connect",
    disconnect: "Disconnect",
    edit: "Edit",
    delete: "Delete",
    submitNewTask: "Submit New Task to This Server",
    connectFailed: "Connection failed",
    taskList: "Remote Tasks",
    noTasks: "No tasks on this server",
    refresh: "Refresh",
    cancelTask: "Cancel task",
    pauseTask: "Pause task",
    resumeTask: "Resume task",
    server: {
      title: "Remote Service",
      hint: "Once enabled, clients can connect with a token and submit download tasks",
      start: "Enable",
      stop: "Disable",
      toggling: "Switching…",
      running: "Service Running",
      port: "Listen Port",
      portHint: "Can be changed while server is stopped",
      lanAddress: "LAN Address",
      firewallHint:
        "Make sure your firewall allows inbound TCP connections on port {port}",
      tokenTitle: "Access Token",
      tokenEmpty: "No token generated yet",
      tokenHint: "Clients must provide this token to connect",
      copy: "Copy",
      copied: "Copied",
      generate: "Generate New Token",
      generating: "Generating…",
      clients: "Connected Clients",
      noClients: "No clients connected",
      switchHint:
        "Switching to client mode will automatically stop the remote service",
    },
    switchDialog: {
      title: "Switch Mode",
      bodyToClient:
        "Switching to client mode will immediately stop the remote server and disconnect all connected clients. You will need to re-enable it manually if you switch back.",
      bodyToServer:
        "Switching to server mode will disconnect from the current remote server.",
      confirm: "Confirm Switch",
      cancel: "Cancel",
    },
    addForm: {
      title: "Add Remote Server",
      editTitle: "Edit Server",
      namePlaceholder: "e.g. Studio Desktop",
      url: "Server URL",
      token: "Access Token",
      save: "Save",
      cancel: "Cancel",
      validationError: "Server URL and Token are required",
    },
    status: {
      queued: "Queued",
      downloading: "Downloading",
      paused: "Paused",
      completed: "Completed",
      error: "Error",
      cancelled: "Cancelled",
    },
  },

  about: {
    title: "About 御图",
    subtitle: "Offline Map Tile Manager",
    stableVersion: "Stable",
    featuresTitle: "Key Features",
    features: [
      "Supports TMS / XYZ / WMTS tile sources",
      "Multi-threaded concurrent download with resume",
      "Export to MBTiles or directory structure",
      "Built-in TMS / WMTS publish server",
      "Supports .lrc / .lra / .ovmap layer config files",
      "Supports Tianditu, StarMap, and custom tile sources",
    ],
    openSourceTitle: "Open Source",
    githubLabel: "GitHub Repository",
    licenseLabel: "License",
    copyright: "© 2026 TileGrabber Contributors",
    mapDataNote:
      "Map data is copyrighted by the respective data providers. Please ensure you have obtained the appropriate authorization before use.",
  },

  help: {
    title: "Help Documentation",
    appDesc:
      "Cross-platform map tile download tool supporting TMS / WMTS / .lrc / .lra data sources, with resume download, anti-ban features, MBTiles export, and a local tile publish server (XYZ / WMTS / WMS / OGC API Tiles / ArcGIS REST).",
    version: "御图 v{version} · Built with Tauri v2 + MapLibre GL JS v4",
    sections: [
      {
        title: "Quick Start",
        items: [
          {
            q: "How to create a download task?",
            a: 'Click the "Tasks" tab in the top navigation bar, then click "New Download Task" in the left panel. In the wizard, select a map source (.lrc / .lra / .ovmap local file, WMTS URL, or TMS/XYZ URL). After previewing, draw the target area on the map, set the zoom level range, and click "Start Download".',
          },
          {
            q: "How to draw a download area on the map?",
            a: 'After selecting a map source in the wizard, the left panel shows "Rectangle" and "Polygon" drawing mode options. Select a mode, then click the draw button in the top-left corner of the map to activate drawing. Rectangle: drag to select; Polygon: click each vertex, double-click or click the start point to close. The area will be precisely clipped to the polygon boundary. After drawing, adjust the zoom range and preview the tile count in the left panel.',
          },
          {
            q: "How to check download progress?",
            a: "Active download tasks show a real-time progress bar and speed in the left task list. Click the detail button on a task card to open the task detail panel, showing real-time speed (tiles/sec and MB/sec), ETA, downloaded/failed tile count, and run logs.",
          },
        ],
      },
      {
        title: "Map Layer Management",
        items: [
          {
            q: "What are layers?",
            a: 'When the "Map" tab is active, the left panel shows a list of saved layers. A layer is a saved map source configuration that can be quickly previewed or used as a download task source — without re-entering URLs or re-importing files each time.',
          },
          {
            q: "How to add a layer?",
            a: 'Click "Add Layer" in the left panel of the Map tab, select a data source through the wizard, and save. Layers are persisted and remain available after restarting the application.',
          },
          {
            q: "How to preview a layer?",
            a: "Click a layer in the layer list to overlay its tiles on the map for a preview. Click the same layer again or switch to another tab to cancel the preview.",
          },
          {
            q: "How to create a download task from a layer?",
            a: "Click the download icon on a layer card. The app will switch to map view and enter download configuration mode using that layer's source. Draw the area and start the download.",
          },
        ],
      },
      {
        title: "Supported Data Sources",
        items: [
          {
            q: ".lrc files",
            a: "Locaspace Viewer layer config files containing TMS/WMTS URL templates, extent, and zoom info. GB18030 encoding is automatically decoded.",
          },
          {
            q: ".lra files",
            a: "Gzip-compressed .lrc (or JSON-format 3D Tiles index). The tool auto-detects and parses the internal format.",
          },
          {
            q: ".ovmap files",
            a: "Layer config files exported from OvitalMap. The tool automatically parses the internal tile URL template and extent info.",
          },
          {
            q: "WMTS URL",
            a: "Enter a standard OGC WMTS 1.0.0 GetCapabilities document URL. The tool auto-parses layer list, extent, and zoom levels and lets you select the desired layer.",
          },
          {
            q: "TMS / XYZ URL",
            a: "Enter a tile URL template containing {z}, {x}, {y} placeholders, e.g. https://tile.openstreetmap.org/{z}/{x}/{y}.png.",
          },
        ],
      },
      {
        title: "Task Management",
        items: [
          {
            q: "Pause and Resume",
            a: 'Click the "Pause" / "Resume Download" button in the task detail panel. Already-downloaded tiles are preserved. Resuming continues from where it left off without re-downloading.',
          },
          {
            q: "Retry Failed Tiles",
            a: 'After download completes with failures, the detail panel shows a "Retry N Failed Tiles" button that re-requests only the failed items.',
          },
          {
            q: "Delete Task",
            a: 'Click "Delete Task" in the task detail panel. The deletion will only proceed after confirming the secondary confirmation prompt. Note: the corresponding tile storage files will also be deleted and cannot be recovered.',
          },
          {
            q: "Locate Task on Map",
            a: "Click the locate button in the detail panel header to fly the map to the task download boundary, with downloaded tiles overlaid on the map.",
          },
          {
            q: "Export / Import Task Package (.tgr)",
            a: 'Click the "Export" button on a task card to package that task together with all downloaded tiles as a .tgr file (SQLite format, extremely fast zero-copy). The "Import" button at the top of the task list loads a .tgr file shared by others — the program reads it directly, so do not move or delete the file. External .tgr task cards have a special identifier, and you can optionally delete the file when removing the task.',
          },
        ],
      },
      {
        title: "Export",
        items: [
          {
            q: "MBTiles",
            a: "Standard SQLite tile container, compatible with QGIS, MapTiler, Mapbox, etc. Tile row numbers conform to TMS specification (y-axis flipped).",
          },
          {
            q: "Directory Structure (z/x/y)",
            a: "Exports as a z/x/y.{ext} directory tree, suitable for self-hosting or direct use with Leaflet.js.",
          },
          {
            q: "GeoTIFF",
            a: "Merges downloaded tiles and exports as a georeferenced GeoTIFF raster file that can be overlaid directly in QGIS/ArcGIS. Polygon tasks are automatically clipped to the boundary at pixel precision. Very large extents automatically use BigTIFF format (supports >4 GB files).",
          },
          {
            q: "How to export?",
            a: 'After download completes (or is in progress), click "Export Tiles" in the task detail panel, select a format and output path, then click "Start Export".',
          },
        ],
      },
      {
        title: "Local Publish Server",
        items: [
          {
            q: "How to start the publish server?",
            a: 'Click the "Publish" navigation at the top, set the port number, and click "Start Server". Only fully downloaded tasks appear in the publish list.',
          },
          {
            q: "Which protocols are supported?",
            a: "Each task is simultaneously served via five standard protocols: XYZ (TMS), WMTS 1.0.0, WMS 1.1.1, OGC API Tiles, and ArcGIS REST API. Switch between them using the protocol buttons in the panel.",
          },
          {
            q: "XYZ (TMS) Endpoint",
            a: "http://localhost:{port}/tiles/{task_id}/{z}/{x}/{y}  —  Ready to use with Leaflet, MapLibre GL JS, OpenLayers, etc.",
          },
          {
            q: "WMTS Capabilities Document",
            a: "http://localhost:{port}/wmts/{task_id}?SERVICE=WMTS&REQUEST=GetCapabilities  —  Loadable directly by QGIS, ArcGIS, Cesium, and other GIS tools.",
          },
          {
            q: "WMS Service",
            a: "GetCapabilities: http://localhost:{port}/wms/{task_id}?SERVICE=WMS&REQUEST=GetCapabilities  —  Supports EPSG:4326 and EPSG:3857 projections. GetMap requests dynamically composite tile images on the fly.",
          },
          {
            q: "OGC API Tiles",
            a: "http://localhost:{port}/ogc/{task_id}/tiles/WebMercatorQuad/{z}/{y}/{x}  —  Compatible with MapLibre GL JS and other modern clients via a REST-style tile API.",
          },
          {
            q: "ArcGIS REST API",
            a: "http://localhost:{port}/arcgis/rest/services/{task_id}/MapServer  —  Compatible with the Esri ecosystem. Mount directly in ArcGIS Online, Esri Leaflet, or the ArcGIS API for JavaScript.",
          },
          {
            q: "LAN (Local Network) Access",
            a: 'When the server starts, the "Access Address" dropdown lists all detected LAN IP addresses. Selecting one instantly updates all service URLs and code examples in the panel so other devices can connect.',
          },
          {
            q: "Built-in Code Examples",
            a: 'Expand the "Code Example" section for any task to view ready-to-use integration code for Cesium.js, Leaflet.js, or MapLibre GL JS — copy with one click.',
          },
          {
            q: "Request Statistics",
            a: "The panel shows live request counts per protocol (XYZ / WMTS / WMS) and the last request time at the bottom of each task row, making it easy to monitor usage.",
          },
          {
            q: "Using TMS in QGIS",
            a: 'Open "Data Source Manager → XYZ Tiles", create a new connection, enter the TMS URL, replacing {z}/{x}/{y} with {0}/{1}/{2}.',
          },
        ],
      },
      {
        title: "Download Settings",
        items: [
          {
            q: "Concurrency",
            a: "Default is 32, adjustable in Settings. For most tile sources, 16–32 is optimal; exceeding 64 is likely to trigger 429 rate limiting.",
          },
          {
            q: "Request Interval",
            a: "Random delay between requests, default 0–150 ms. Moderate jitter reduces ban risk; setting to 0 gives maximum speed but with slightly higher risk.",
          },
          {
            q: "Download Rules (Time Window / Rate Limit)",
            a: 'In "Settings → Download Rules" you can configure allowed download time periods (e.g., only at night) and maximum tiles per second, suitable for scenarios with bandwidth limits or access policy restrictions.',
          },
          {
            q: "Strict Clipping & Streaming Clip Pipeline",
            a: 'Enable "Strict clip to task bounds" when creating a task to pixel-clip boundary tiles after download, making transparent pixels outside the task area so edges in exports/previews stay clean. Raster + non-GCJ02 tasks automatically use the "streaming clip" pipeline: boundary tiles are clipped concurrently as they download, so when the download finishes the result is ready — no separate "clipping" stage. GCJ02 sources (AMap / Tencent) still run the serial post-download clip because they require pixel offset compositing first. Vector tiles (pbf / mvt) skip pixel-level clipping entirely.',
          },
        ],
      },
    ],
  },

  tasks: {
    title: "Download Tasks",
    createTask: "New Download Task",
    importPkg: "Import Package",
    importMbtiles: "Import MBTiles",
    toast: {
      completed: '"{name}" download complete',
      completedWithErrors: '"{name}" finished with errors',
      completedWithErrorsHint: "Some tiles failed — check task details",
      failed: '"{name}" download failed',
    },
    filterAll: "All",
    filterDownloading: "Active",
    filterCompleted: "Done",
    filterFailed: "Failed",
    emptyTitle: "No Tasks",
    emptyDesc: 'Click "New Download Task" to get started',
    exportPkg: "Export Package",
    exportPkgTitle: "Export Task Package",
    exportPkgFilter: "御图 Task Package",
    importPkgTitle: "Import Task Package",
    exportSuccess:
      "Export successful!\n\nNote: The program will read this file directly after import. Do not move or delete it:\n{path}",
    exportFailed: "Export failed: {error}",
    importFailed: "Import failed: {error}",
    confirmCancel:
      "Task progress will be preserved. You can restart at any time.",
    confirmCancelTitle: "Cancel this task?",
    confirmDeleteInternal:
      "The task and all downloaded tiles will be permanently deleted. This action cannot be undone.",
    confirmDeleteExternal:
      "The task record will be deleted. This task uses an external file — you will need to decide separately whether to delete the .tgr file.",
    confirmDeleteTitle: "Delete this task?",
    confirmDeleteFileTitle: "Also delete external file?",
    confirmDeleteFile: "Also delete the .tgr file?\n{path}",
    batchModeOn: "Batch select",
    batchModeOff: "Exit batch mode",
    selectAll: "Select all",
    deselectAll: "Deselect all",
    selected: "{count} selected",
    batchPause: "Pause selected",
    batchResume: "Resume selected",
    batchDelete: "Delete selected",
    confirmBatchDelete:
      "{count} tasks and all their downloaded tiles will be permanently deleted. This cannot be undone.",
    confirmBatchDeleteTitle: "Delete {count} tasks?",
    diskWarning: "Disk space may be insufficient",
    diskWarningDetail:
      "Estimated {estimated} MB needed, {available} MB available.",
  },

  taskCard: {
    externalBadge: "External",
    externalTitle:
      "This task data comes from an external file. Do not move or delete the .tgr file.",
    remoteBadge: "Remote",
    remoteBadgeTitle: "This task was submitted by a remote client",
    tiles: "tiles",
    pausing: "Pausing…",
    pause: "Pause",
    resume: "Resume",
    export: "Export",
    retry: "Retry",
    delete: "Delete",
    exporting: "Exporting",
    cancelExport: "Cancel export",
    failedBadge: "⚠ {count} failed",
    status: {
      downloading: "Downloading",
      paused: "Paused",
      completed: "Completed",
      completed_with_errors: "Done (with errors)",
      failed: "Failed",
      pending: "Pending",
      processing: "Clipping",
    },
    remaining: "{time} left",
    retryingIn: "Retrying in {secs}s",
  },

  taskDetail: {
    back: "Back",
    locateOnMap: "Locate on Map",
    downloaded: "Downloaded",
    failed: "Failed",
    total: "Total",
    tilesPerSec: "tiles/s",
    exporting: "Exporting — {format}",
    exportHistory: "Export History",
    openLocation: "Show in Explorer",
    zoomRange: "Zoom Range",
    bbox: "Bounding Box",
    createdAt: "Created",
    updatedAt: "Updated",
    clipping: "Clipping",
    pauseDownload: "Pause Download",
    pausing: "Pausing…",
    resumeDownload: "Resume Download",
    exportTiles: "Export Tiles",
    retryFailed: "Retry {count} Failed Tiles",
    failedView: {
      title: "Failed Tile Distribution",
      hint: "Click a zoom level to highlight failed tiles on the map; retry individually or in bulk.",
      previewing: "previewing",
      retryThisLevel: "Retry this level",
      selectAll: "Select all",
      retrySelected: "Retry selected ({count})",
      retrying: "Retrying…",
    },
    deleteTask: "Delete Task",
    confirmDelete:
      "Delete this task? All tile data will be cleared. This action cannot be undone.",
    confirmDeleteBtn: "Confirm Delete",
    deleting: "Deleting…",
    cancelDelete: "Cancel",
    status: {
      pending: "Pending",
      downloading: "Downloading",
      processing: "Clipping",
      paused: "Paused",
      completed: "Completed",
      completed_with_errors: "Done (errors)",
      failed: "Failed",
      cancelled: "Cancelled",
    },
    exportFormat: {
      mbtiles: "MBTiles",
      directory: "Directory (z/x/y)",
      tiff: "GeoTIFF",
    },
    incremental: {
      title: "Incremental Update",
      extend: "Extend Task",
      import: "Borrow Tiles",
      importTitle: "Import tiles from another task",
      importDesc:
        "Copy tiles from a fully-completed task with the same source (URL / format / CRS) to skip network downloads.",
      importNoneFound: "No compatible completed source task found.",
      importReusable: "Reusable tiles: {count}",
      importCounting: "Counting reusable tiles…",
      importConfirm: "Import tiles",
      importedHint:
        "Imported {imported} tiles (pending before: {pendingBefore}).",
      previewHint: "Pick a task on the left to preview",
      close: "Close",
      running: "Running…",
    },
  },

  extendMode: {
    banner: "Extending: {name}",
    hint: 'Draw the additional area on the map, then click "Confirm extend".',
    confirm: "Confirm extend",
    cancel: "Cancel",
    running: "Processing…",
    errorNoArea: "Please draw the additional area on the map first.",
    success: "Added {added} tiles to the task queue.",
  },

  publish: {
    title: "Local Publish Server",
    port: "Port",
    startServer: "Start Server",
    stopServer: "Stop Server",
    running: "Running :{port}",
    stopped: "Stopped",
    noTasks: "No publishable tasks (tasks must be fully downloaded first)",
    readyTasks: "Publishable tasks ({count})",
    copyXyz: "Copy XYZ URL",
    copyWmts: "Copy WMTS URL",
    copyWms: "Copy WMS URL",
    copyOgc: "Copy OGC Tiles URL",
    copyArcgis: "Copy ArcGIS REST URL",
    accessAddr: "Access address",
    codeExample: "Code Example",
    copy: "Copy",
    copied: "Copied",
    copyCode: "Copy code",
    stats: {
      title: "Requests",
      noRequests: "No requests yet",
      justNow: "Just now",
      minutesAgo: "{n} min ago",
      hoursAgo: "{n} hr ago",
    },
  },

  progress: {
    status: {
      downloading: "Downloading",
      processing: "Clipping",
      paused: "Paused",
      completed: "Completed",
      completed_with_errors: "Done (with errors)",
      failed: "Failed",
      cancelled: "Cancelled",
      pending: "Pending",
    },
    clipped: "Clipped {done} / {total} tiles",
    tiles: "{done} / {total} tiles",
    failed: "({count} failed)",
    pauseBtn: "Pause",
    resumeBtn: "Resume",
    cancelBtn: "Cancel",
    confirmCancel: "Cancel this download task?",
    keepDownloading: "Keep Downloading",
    doCancel: "Cancel Download",
  },

  export: {
    title: "Export Tiles",
    tiles: "{count} tiles total",
    selectFormat: "Select Export Format",
    mbtitlesDesc: "Single SQLite file, compatible with QGIS, Cesium, etc.",
    directoryDesc: "File tree, usable as a static XYZ tile server.",
    tiffDesc: "Georeferenced raster, supported by QGIS/ArcGIS.",
    pmtilesDesc: "Modern single-file format, served directly via HTTP/CDN.",
    pmtilesWgs84Hint: "PMTiles only supports WebMercator; this task uses WGS84",
    savePath: "Save Path",
    exportDir: "Export Directory",
    noPath: "No path selected…",
    noDir: "No directory selected…",
    browse: "Browse…",
    mergeZoom: "Merge Zoom Level",
    mergeZoomDesc:
      "z{min}–z{max} available — higher zoom means sharper but larger file",
    compression: "Compression",
    compressionNone: "None",
    outputCrs: "Output CRS",
    clipOption: "Strictly clip to task bounding box",
    clipTiffDesc:
      "Clip output pixels precisely to task bounding box coordinates",
    clipDefaultDesc:
      "Only export tiles fully within the task bounding box, excluding edge-intersecting tiles",
    reEncodeLabel: "Re-encode images (compression/quality)",
    reEncodeDesc: "Re-encode tiles to adjust file size — increases export time",
    vectorNotice:
      "Vector tile task: export will gzip-wrap tiles and write vector_layers metadata per the MBTiles/PMTiles spec. Pixel-level clipping and image re-encoding don't apply and are hidden.",
    vectorNoTiff:
      "Vector tiles can't be exported as GeoTIFF (no raster pixels)",
    jpegQuality: "JPEG Quality",
    pngLevel: "PNG Level",
    startExport: "Start Export",
    cancel: "Cancel",
    exporting: "Exporting…",
    exportDone: "Export Complete",
  },

  downloadSetup: {
    title: "New Download Task",
    taskName: "Task Name",
    taskNamePlaceholder: "Enter task name",
    dataSource: "Data Source",
    drawArea: "1 · Draw Download Area",
    zoomLevel: "2 · Select Zoom Levels",
    downloadOptions: "3 · Download Options",
    drawRectangle: "Rectangle",
    drawPolygon: "Polygon",
    importFile: "Import",
    importAreaTitle: "Import Area File",
    importAreaFilter: "Area Files (KML, KMZ, GeoJSON)",
    areaSelected: "✓ Area selected",
    clearArea: "Clear",
    rectPrompt: "Drag on the map to select the geographic area to download.",
    polyPrompt:
      "Click vertices on the map one by one. Double-click to close the polygon.",
    startRect: "Start Selection",
    startPoly: "Start Drawing",
    cancelDraw: "Click to cancel",
    clipOption: "Strictly clip to selection",
    clipDesc:
      "Precisely clip edge tiles to the selection boundary when exporting, avoiding overflow area data",
    startDownload: "Start Download",
    submitRemote: "Submit to Remote",
    targetLocal: "Download locally",
    targetRemote: "Remote — {name}",
    cancel: "Cancel",
    estimateTiles: "~{count} tiles",
    crsLabels: {
      WEB_MERCATOR: "EPSG:3857",
      WGS84: "EPSG:4326",
      TERRAIN: "Terrain Elevation",
      UNKNOWN: "Unknown",
    },
    returnToList: "Back to Task List",
  },

  rules: {
    timeWindow: "Download Time Window",
    timeWindowDesc:
      "Only run downloads during specified time periods. Ideal for large downloads during off-peak night hours to avoid consuming daytime bandwidth.",
    enabled: "Enabled",
    disabled: "Disabled",
    startTime: "Start Time",
    endTime: "End Time",
    windowRange: "Will download between {range}",
    crossMidnight: "(crosses midnight)",
    rateLimit: "Download Rate Limit",
    rateLimitDesc:
      "Limit the maximum number of tiles downloaded per second to prevent overly dense requests from triggering server bans. Set to 0 for unlimited.",
    noLimit: "Unlimited",
    rateValue: "≤ {count} tiles/sec",
    burstPause: "Burst Pause Duration (ms)",
    burstPauseDefault: "Random 600–2200ms",
    burstPauseValue: "{ms} ms",
    burstPauseDesc:
      "Wait time after each batch of tiles, simulating user map panning behavior. 0 = use built-in random pause (recommended).",
    auto: "Auto",
  },

  zoom: {
    levelRange: "Zoom Range",
    min: "Min",
    max: "Max",
    estimatedTiles: "Estimated Tile Count",
    noArea: "Please draw a download area on the map first",
    calcFailed: "Unable to calculate tile count",
    warnMany: "High tile count — download may take a long time",
    errorTooMany:
      "Tile count too large. Please reduce the area or lower the max zoom level.",
  },

  wizard: {
    selectSource: "Select Data Source",
    step1Desc: "Select source type and parse",
    step2Desc: "Select layer to download",
    sourceFile: "Local File",
    sourceFileDesc: ".lrc / .lra / .ovmap layer resource files",
    sourceWmts: "WMTS Service",
    sourceWmtsDesc: "Enter GetCapabilities URL",
    sourceTms: "TMS/XYZ URL",
    sourceTmsDesc: "Enter tile URL template",
    sourceWeb: "Web Capture",
    sourceWebDesc: "Intercept actual browser requests",
    sourceMbtiles: "MBTiles File",
    sourceMbtilesDesc: "Import local .mbtiles offline package",
    sourcePreset: "Preset Maps",
    sourcePresetDesc: "Popular built-in basemaps",
    presetPreviewHint: "Click Confirm to create a download task",
    presetTokenHint: "This basemap requires {label}",
    presetTokenPlaceholder: "Paste your token",
    presetTokenApply: "Apply for one",
    selectFileTitle: "Select Layer Resource File",
    selectFileFilter: "Layer Resources",
    urlPlaceholder: "Enter URL",
    wmtsUrlPlaceholder: "Enter WMTS GetCapabilities URL",
    tmsUrlPlaceholder:
      "Enter tile URL template, e.g. https://tile.example.com/{z}/{x}/{y}.png",
    webUrlPlaceholder: "Enter webpage URL to capture",
    customName: "Custom Name (optional)",
    customNamePlaceholder: "Auto-extracted from URL if left empty",
    startCapture: "Open Capture Window",
    stopCapture: "Finish Capture",
    capturing: "Capturing…",
    capturedCount: "{count} layers captured",
    noCaptured:
      "No tile URLs captured. Please open the capture window and browse the map, then try again.",
    errNoWmtsUrl: "Please enter a WMTS service URL",
    errNoTmsUrl: "Please enter a tile URL template",
    errNoWebUrl: "Please enter a webpage URL to capture",
    errNoLayers: "WMTS service returned no layers",
    next: "Next",
    confirm: "Confirm",
    cancel: "Cancel",
    parse: "Parse",
    back: "Back",
    layerSelect: "Select Layer",
    preview: "Preview",
    filePickerTitle: "Click to select .lrc, .lra or .ovmap file",
    filePickerDesc:
      "Layer resources exported from OrbitGIS, EasyEarth, LocaSpaceViewer, etc.",
    mbtilesPickerTitle: "Click to select .mbtiles file",
    mbtilesPickerDesc:
      "Import MBTiles offline package as data source — tiles will be written to the task store",
    wmtsHint:
      "Enter the WMTS service capabilities URL to automatically retrieve the layer list",
    tmsLabel: "Tile URL Template",
    customNameLabel: "Layer Name (optional)",
    webUrlLabel: "Webpage URL",
    webHint:
      "After clicking 'Start Capture', a browser window will open. Browse the map to trigger tile requests, then click 'Finish Capture'.",
    captureWaiting:
      "Browse the map in the popup browser window and wait for tiles to appear in this list",
    requestConfig: "Request Config",
    httpHeaders: "HTTP Headers",
    add: "Add",
    noHeaders:
      "No headers yet. Click 'Add' to set Referer, Authorization, etc.",
    dynamicParams: "Dynamic Parameter Scripts",
    dynamicParamsHint: "Use in URL template as",
    paramNameExample: "param",
    dynamicParamsHintPost:
      "to reference; scripts are JS expressions evaluated before downloading",
    dynamicParamsEmpty:
      "e.g. name: ts, script: Date.now(), reference as {ts} in URL",
    paramName: "Name",
    paramNamePlaceholder: "Param name (e.g. token)",
    jsExpr: "JS Expression",
    layerPreview: "Layer Preview",
    webFoundPrefix: "Found in page:",
    webFoundSuffix: "tile service(s), please select:",
    wmtsContainsPrefix: "This WMTS service contains",
    wmtsContainsSuffix: "layer(s), please select:",
    advancedConfig: "Advanced Request Config",
    headers: "Request Headers",
    addHeader: "Add Header",
    headerKey: "Key",
    headerValue: "Value",
    paramScripts: "Parameter Scripts",
    addScript: "Add Script",
    scriptName: "Param Name",
    scriptCode: "Script (JS expression)",
  },

  floatWindow: {
    downloadSpeed: "Download Speed",
  },
};
