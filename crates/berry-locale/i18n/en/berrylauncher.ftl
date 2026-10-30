# BerryLauncher interface strings: English (fallback language).
#
# Adding a language: create i18n/<code>/berrylauncher.ftl (for example i18n/fr/ or i18n/pt-BR/),
# translate the values and fill in the metadata block. No code changes are needed; the file is
# embedded at build time and offered in Preferences automatically.
#
#   language-name  - name shown in the language list, in that language
#   hoyo-language  - language code used by HoYoverse launcher backgrounds (en-us, es-es, fr-fr...)
#   play-language  - Google Play language code (hl) for store screenshots (en, es, fr...)
#   play-country   - Google Play country code (gl) for store screenshots (US, ES, FR...)
#   wine-locale    - locale exported to Wine/Proton prefixes while the launcher is in this language
#                    (en_US.UTF-8, es_ES.UTF-8...)
#
# Keep { $variables } unchanged. Messages missing from a translation fall back to English.

## metadata
language-name = English
hoyo-language = en-us
play-language = en
play-country = US
wine-locale = en_US.UTF-8

## nav
nav-games = Games
nav-store = Store
nav-runners = Runners

## menu
menu-preferences = Preferences
menu-main = Main Menu
menu-open-folder = Open Launcher Folder
menu-close-launcher = Close BerryLauncher

## preferences
preferences-appearance = Appearance
preferences-theme-system = System
preferences-theme-light = Light
preferences-theme-dark = Dark
preferences-language-title = Language
preferences-language-system = System
preferences-language-subtitle = Language changes are applied after restarting BerryLauncher.
preferences-launcher = Launcher
preferences-behavior-hide = Hide launcher
preferences-behavior-title = Launcher behaviour when game runs
preferences-behavior-subtitle = Choose what BerryLauncher should do when a game starts.
preferences-behavior-keep-open = Keep it open
preferences-behavior-close = Close
preferences-animated-background-title = Use animated background for games
preferences-animated-background-subtitle = Use an official animated background when a game provides one.
preferences-restore-game-title = Reopen the last game on startup
preferences-restore-game-subtitle = Start on the page of the game you were viewing last time instead of the games list.
preferences-downloads = Downloads
preferences-quit-confirm-title = Confirm before closing during downloads
preferences-quit-confirm-subtitle = Ask first when closing BerryLauncher while a game is being installed, updated or repaired.
preferences-pause-on-launch-title = Pause downloads while playing
preferences-pause-on-launch-subtitle = Pause running downloads when a game starts so they do not compete with it for bandwidth and disk. Resume them from the game page.
quit-confirm-heading = Close BerryLauncher?
quit-confirm-body = A download is in progress. It will be paused and can be resumed next time.
quit-confirm-close = Close

## common
common-cancel = Cancel
common-remove = Remove
common-add = Add

## library
library-count = { $count ->
    [one] { $count } game in your library
   *[other] { $count } games in your library
}
library-filtered-count = { $count } of { $total } games in your library
library-back = Back to Library
library-remove = Remove from Library
library-remove-confirm-title = Remove game from Library?
library-remove-confirm-detail = The game folder is not empty. Everything inside it will be deleted.
library-remove-error = Could not remove game folder

## sorting
sort-a-z = Sorted A to Z. Click to sort Z to A
sort-z-a = Sorted Z to A. Click to sort A to Z

## store
store-add-to-library = Add to Library
store-title = Discover games
store-subtitle = A growing catalogue of games ready to be added to your Library.
store-back = Back to Store

## game
game-about = About this game
game-developer = Developer
game-publisher = Publisher
game-tags = Tags
game-play = Play
game-install = Install
game-pause = Pause
game-resume = Resume
game-running = Running
game-starting = Starting…
game-launch = Launch game
game-action-tooltip = Launch, install or resume the game
game-running-tooltip = Game is running
game-pause-download = Pause download
game-resume-download = Resume download
game-pause-update = Pause update
game-resume-update = Resume update
game-update = Update game
game-settings-tooltip = Game settings
game-error-download = Could not complete game download
game-error-launch = Could not launch game
game-error-update = Could not update game
game-error-runners = Could not initialize runners
game-resuming = Resuming…
game-preparing = Preparing…
game-preparing-update = Preparing update…

## progress
progress-paused = Paused
progress-verifying = Verifying
progress-downloading = Downloading
progress-installing = Installing
progress-validating = Validating
progress-finishing = Finishing
progress-preparing = Preparing

## settings
settings-title = Game Settings
settings-runtime = Runtime
settings-gamemode-title = Enable Feral GameMode
settings-gamemode-subtitle = Temporarily applies system performance optimizations while the game is running
settings-dxvk-hud-title = DXVK HUD
settings-dxvk-hud-dx12-hint = This option is only available with DirectX 11. For DirectX 12, you can use MangoHud as an alternative.
settings-dxvk-hud-subtitle = Choose which DXVK performance information is shown in-game
settings-dxvk-hud-none = None
settings-dxvk-hud-fps = FPS
settings-dxvk-hud-fps-frametime = FPS + Frametime
settings-dxvk-hud-fps-frametime-gpu = FPS + Frametime + GPU
settings-dxvk-hud-full = Full
settings-graphics-api-title = Graphics API
settings-graphics-api-subtitle = DirectX 12 is the game's default; switch to DirectX 11 if you have rendering problems or crashes
settings-graphics-api-dx12 = DirectX 12 (default)
settings-graphics-api-dx11 = DirectX 11
settings-fsr-title = FSR (AMD FidelityFX Super Resolution)
settings-fsr-subtitle = Upscales the game when it runs at a lower resolution than your screen (needs a runner with the Wine fullscreen hack, such as Proton-GE)
settings-fsr-quality-title = FSR quality
settings-fsr-quality-subtitle = Preset used by the upscaler
settings-fsr-ultra = Ultra quality
settings-fsr-quality = Quality
settings-fsr-balanced = Balanced (default)
settings-fsr-performance = Performance
settings-sync-title = Wine synchronization
settings-sync-subtitle = NTSync is used by default when your system supports it, otherwise FSync. The selected and the actually used primitive are written to the game log
settings-sync-ntsync = NTSync
settings-sync-fsync = FSync
settings-sync-esync = ESync
settings-sync-none = None
settings-gamescope-title = Gamescope
settings-gamescope-subtitle = Run the game inside gamescope
settings-gamescope-missing-native = Gamescope was not found. Install it with your distribution's package manager (for example: sudo dnf install gamescope)
settings-gamescope-missing-host = Gamescope was not found on the host. Install it with your distribution's package manager (for example: sudo dnf install gamescope)
settings-gamescope-missing-sandbox = Not available inside the Flatpak sandbox. Install the extension (flatpak install flathub org.freedesktop.Platform.VulkanLayer.gamescope) or turn on "Launch on the host" to use the system's gamescope
settings-host-title = Run in the Flatpak sandbox
settings-host-subtitle = Flatpak only. On: the game runs inside the sandbox. Off: it starts on the system instead (same as BERRY_LAUNCH_ON_HOST=1), which also lets it use the system's gamescope
settings-host-forced = BERRY_LAUNCH_ON_HOST={ $value } is set in the environment: the toggle is ignored and that value is followed.
settings-gamescope-configure = Gamescope settings
gamescope-title = Gamescope settings
gamescope-display-group = Display
gamescope-auto-hint = 0 = automatic
gamescope-width = Output width
gamescope-height = Output height
gamescope-game-width = Game width
gamescope-game-height = Game height
gamescope-fps = FPS limit
gamescope-fps-unfocused = FPS limit when unfocused
gamescope-mode-title = Window mode
gamescope-mode-fullscreen = Fullscreen
gamescope-mode-borderless = Borderless
gamescope-mode-windowed = Windowed
gamescope-scaling-group = Upscaling
gamescope-filter-title = Upscaling filter
gamescope-filter-default = Default
gamescope-filter-fsr = FSR
gamescope-filter-nis = NIS
gamescope-filter-linear = Linear
gamescope-filter-nearest = Nearest
gamescope-sharpness = Sharpness
gamescope-sharpness-subtitle = 0 (sharpest) to 20 (softest). Used by FSR and NIS
gamescope-integer = Integer scaling
gamescope-integer-subtitle = Only scale by whole numbers, keeping pixels sharp
gamescope-misc-group = Other
gamescope-hdr = HDR
gamescope-grab = Force grab cursor
gamescope-extra = Extra arguments
gamescope-extra-placeholder = e.g. --mangoapp
settings-fps-title = Enable FPS unlocker
settings-fps-subtitle = Unlock the in-game frame rate limit
settings-fps-target-title = Target FPS
settings-fps-target-subtitle = Applied by the FPS unlocker at game launch
settings-fps-custom = Custom
settings-fps-custom-title = Custom FPS
settings-fps-custom-subtitle = Exact frame rate to apply instead of a preset
settings-fps-refresh-title = Refresh delay (ms)
settings-fps-refresh-subtitle = How often the unlocker re-applies the FPS value while the game runs
settings-fps-init-title = Initial delay (ms)
settings-fps-init-subtitle = How long the unlocker waits after the game starts before looking for its FPS setting
settings-compatibility = Compatibility
settings-runner = Runner
settings-remake-prefix-title = Remake prefix
settings-remake-prefix-subtitle = Recreate the prefix for the selected runner
settings-remake-prefix-button = Remake
settings-installation = Installation
settings-location-title = Game location
settings-location-choose = Choose folder
settings-location-dialog = Choose Game Location
settings-repair-title = Repair Game
settings-repair-subtitle = Verify the installed files and redownload damaged or missing data
settings-repair-running = Repairing…
settings-repair-done = Repair completed
settings-repair-done-detail = The game files were verified and repaired successfully.
settings-repair-failed = Repair failed
settings-env-title = Environment Variables
settings-env-add = Add Environment Variable
settings-env-value = Value
settings-env-variable = Variable
settings-env-remove = Remove variable

## status
status-fetching-manifest = Fetching game manifest…
status-fetching-repair-manifest = Fetching repair manifest…
status-installing = Installing { $name } ({ $current }/{ $total })
status-repairing = Repairing { $name } ({ $current }/{ $total })

## runners
runners-title = Compatibility Runners
runners-subtitle = Install and manage the Wine/Proton runners used to launch your games.
runners-show-all = Show All
runners-show-all-count = Show All ({ $count })
runners-cancel = Cancel download

## catalog
catalog-genshin-impact =
    Welcome to Teyvat, a continent filled with countless wonders where countless creatures thrive in harmony.
    
    Ruled by the Seven Archons, this world is a place where the seven elements converge...

    Beneath distant skies, a boy and a girl find themselves caught in the midst of turmoil.
    They are twin siblings, and you will take control of one of them. A deity appears before you, separates you from your sibling, and plunges you into a deep slumber.
    When you awaken, the world is no longer the one you once knew...

    Genshin Impact is an open-world role-playing game in which you will discover the fantastic world of Teyvat.

    You will be able to visit the continent's seven different nations and meet fellow travelers from all over the world to fight powerful enemies together, uncover hidden secrets, and search for your lost sibling.

    Freely explore the different regions of Teyvat and let your curiosity guide you to the continent's most mystical corners! 

catalog-honkai-star-rail = 
    Honkai: Star Rail is HoYoverse's space fantasy RPG.
    Board the Astral Express and discover the infinite wonders of the galaxy on this exciting journey filled with adventure.
    You will meet new companions across various worlds and even encounter familiar faces. Together, you will have to solve the problems caused by the Stellaron and uncover the secrets it holds. May this journey take us all the way to the stars!

    Discover a universe full of surprises
    The jump begins in 3, 2, 1! A Space Station filled with sealed curiosities, a planet trapped in eternal winter, a massive fleet hunting abominations, a festival planet lost in a sweet dream, an eternal land with an uncertain fate, and even an arcadia that has fallen into a painting... Every stop aboard the Astral Express offers a unique glimpse into the galaxy! Explore marvelous worlds and civilizations and uncover unimaginable mysteries on this extraordinary journey!

catalog-zenless-zone-zero =
    «Zenless Zone Zero» is a new 3D action game by HoYoverse set in the near future, where mysterious supernatural phenomena known as «Hollows» have ravaged the world.

    A double identity, two different lives
    In the near future, mysterious supernatural phenomena known as «Hollows» emerge. Amid the catastrophe, New Eridu rises as a city unlike any other. This last oasis possesses the technology needed to ensure coexistence with the Hollows and is home to a host of chaotic, boisterous, dangerous, and highly dynamic factions. As a professional Proxy, you will play a vital role in connecting the city with the Hollows. New Eridu will bear witness to your story.

catalog-honkai-impact-3rd =
    The Dreamseeker has found her first dream and the first light that quietly breaks through. "The form I now take is the ultimate testament."
    
    Honkai Impact 3rd is a sci-fi action-adventure game developed by HoYoverse.

    Cel-shaded 3D graphics, 3D combat with free jumping, endless combos, precise controls... Experience cutting-edge real-time action!

    An original story told across different media, immersive in-game events, and a stellar voice cast... Become part of the legend!

    The crisis on Earth has subsided for now, while a new adventure unfolds on Mars.

    Meet Valkyries with unique personalities and explore the mysteries of Martian civilization alongside them.

catalog-wuthering-waves =
    Wuthering Waves is an open-world action RPG rich in story and offering a high degree of freedom. You awaken as the Rover, accompanied by a vibrant cast of Resonators, on a journey to recover your lost memories, explore the world, and transform it.

    Welcome, Rover.
    The tide recedes, leaving behind a heavy silence.
    Struck by the Lament, humanity suffers as though stranded upon a reef.
    Yet it fights back with a cry that shatters the silence.
    The calamity that brings destruction also heralds rebirth.
    A new world has been born.
    Rover, this is only the beginning of the story—your story.
    Friends to meet, foes to defeat, new powers to acquire, hidden truths to uncover, and spectacles unlike anything ever seen before.
    A vast world of endless possibilities awaits you. But the choice is yours.
    Be the answer, be the leader, and follow the sounds toward a new future.
    We set forth as the tide rises and falls.
    Arise, Rover. This is your odyssey.

## settings pages and groups
settings-page-general = General
settings-page-runner = Runner
settings-page-graphics = Graphics
settings-page-performance = Performance
settings-page-advanced = Advanced
settings-group-performance = GameMode
settings-group-fps = FPS unlocker
settings-group-graphics = Graphics
settings-group-upscaling = Upscaling
settings-group-gamescope = Gamescope
settings-group-sync = Synchronization
settings-sync-detected = NTSync is available on this system and is used by default.
settings-sync-not-detected = NTSync is not available on this system (it needs Linux 6.14+ with the ntsync module), so FSync is used by default.

## game settings
settings-launch-group = Launch
settings-launch-args-title = Launch arguments
settings-launch-args-subtitle = Extra command-line arguments passed to the game executable
settings-launch-args-placeholder = e.g. -popupwindow
settings-wrapper-title = Command prefix
settings-wrapper-subtitle = Program that wraps the whole launch, such as mangohud or taskset -c 0-7
settings-wrapper-placeholder = e.g. mangohud
settings-dedicated-gpu-title = Use the dedicated GPU
settings-dedicated-gpu-subtitle = Asks laptops with hybrid graphics (NVIDIA Optimus, AMD) to render the game on the dedicated GPU
settings-maintenance-group = Maintenance
settings-open-button = Open
settings-open-game-folder-title = Game folder
settings-open-game-folder-subtitle = Open the installation folder in your file manager
settings-open-prefix-title = Wine prefix folder
settings-open-prefix-subtitle = Browse the prefix of the selected runner
settings-prefix-missing = The prefix has not been created yet. It is created the first time you play
settings-open-logs-title = Game logs
settings-open-logs-subtitle = Open the folder with the log of the last launch of every game
settings-shader-cache-title = Clear shader cache
settings-shader-cache-subtitle = Deletes the compiled shaders of this game. They are rebuilt on the next launch, which can cause brief stutter
settings-shader-cache-button = Clear
settings-shader-cache-done = Shader cache cleared
settings-shader-cache-done-detail = { $size } freed.
settings-shader-cache-failed = Could not clear the shader cache
settings-proton-log-title = Proton debug log
settings-proton-log-subtitle = Writes a detailed Wine/Proton log next to the prefix. Useful for bug reports, but slows the game down a little
settings-reset-title = Reset game settings
settings-reset-subtitle = Restores every setting of this game to its default (the game files, the install folder and the runner are kept)
settings-reset-button = Reset
settings-reset-cancel = Cancel
settings-reset-confirm = Reset the settings of this game?
settings-reset-confirm-detail = Every option, including custom environment variables, goes back to its default value.

## tags
tag-action = Action
tag-adventure = Adventure
tag-rpg = RPG
tag-open-world = Open World
tag-turn-based = Turn-based
tag-multiplayer = Multiplayer
tag-fantasy = Fantasy
tag-sci-fi = Sci-Fi
tag-gacha = Gacha

## details window
details-title = Operation details
details-info-tooltip = Show operation details
details-section-overview = Overview
details-section-steps = Steps
details-section-speed = Network speed
details-section-log = Activity
details-row-state = State
details-row-operation = Operation
details-row-version = Target version
details-row-job = Current job
details-row-elapsed = Elapsed time
details-row-backend = Download backend
details-state-running = In progress
details-state-completed = Completed
details-state-paused = Paused
details-state-failed = Failed
details-kind-install = Installation
details-kind-update = Update
details-kind-repair = Repair
details-step-resolving = Fetch download info
details-step-verifying = Verify available files
details-step-downloading = Download new files
details-step-extracting = Extract files
details-step-finalizing = Finalize
details-step-pending = Pending
details-step-skipped = Not needed
details-stat-current = Current
details-stat-average = Average
details-stat-peak = Peak
details-stat-remaining = Remaining
details-stat-downloaded = Downloaded
details-graph-waiting = Waiting for download data…
details-job-none = Not started
details-log-started-install = Installation started
details-log-started-update = Update started
details-log-started-repair = Repair started
details-log-plan = Version { $version } via { $backend }, { $jobs } download job(s)
details-log-job = Started { $name } ({ $current }/{ $total })
details-log-step = Step: { $step }
details-log-completed = Finished successfully
details-log-paused = Paused by the user
details-log-failed = Failed: { $error }
