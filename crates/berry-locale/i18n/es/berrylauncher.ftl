# BerryLauncher interface strings: Spanish

## metadata
language-name = Español
hoyo-language = es-es
play-language = es
play-country = ES
wine-locale = es_ES.UTF-8

## nav
nav-games = Juegos
nav-store = Tienda
nav-runners = Runners

## menu
menu-preferences = Preferencias
menu-main = Menú principal
menu-open-folder = Abrir carpeta del launcher
menu-close-launcher = Cerrar BerryLauncher

## preferences
preferences-appearance = Apariencia
preferences-theme-system = Sistema
preferences-theme-light = Claro
preferences-theme-dark = Oscuro
preferences-language-title = Idioma
preferences-language-system = Sistema
preferences-language-subtitle = El cambio de idioma se aplica al reiniciar BerryLauncher.
preferences-launcher = Launcher
preferences-behavior-hide = Ocultar el launcher
preferences-behavior-title = Comportamiento del launcher al ejecutar un juego
preferences-behavior-subtitle = Elige qué debe hacer BerryLauncher cuando se inicia un juego.
preferences-behavior-keep-open = Mantenerlo abierto
preferences-behavior-close = Cerrar
preferences-animated-background-title = Usar fondo animado para los juegos
preferences-animated-background-subtitle = Usa el fondo animado oficial cuando el juego lo proporciona.
preferences-restore-game-title = Reabrir el último juego al iniciar
preferences-restore-game-subtitle = Empieza en la página del juego que estabas viendo la última vez en lugar de la lista de juegos.
preferences-downloads = Descargas
preferences-quit-confirm-title = Confirmar antes de cerrar durante descargas
preferences-quit-confirm-subtitle = Pregunta antes de cerrar BerryLauncher mientras se instala, actualiza o repara un juego.
preferences-pause-on-launch-title = Pausar descargas mientras juegas
preferences-pause-on-launch-subtitle = Pausa las descargas en curso al iniciar un juego para que no compitan con él por ancho de banda y disco. Reanúdalas desde la página del juego.
quit-confirm-heading = ¿Cerrar BerryLauncher?
quit-confirm-body = Hay una descarga en curso. Se pausará y podrás reanudarla la próxima vez.
quit-confirm-close = Cerrar

## common
common-cancel = Cancelar
common-remove = Eliminar
common-add = Añadir

## library
library-count = { $count ->
    [one] { $count } juego en tu biblioteca
   *[other] { $count } juegos en tu biblioteca
}
library-filtered-count = { $count } de { $total } juegos en tu biblioteca
library-back = Volver a la biblioteca
library-remove = Quitar de la biblioteca
library-remove-confirm-title = ¿Quitar el juego de la biblioteca?
library-remove-confirm-detail = La carpeta del juego no está vacía. Se eliminará todo su contenido.
library-remove-error = No se pudo eliminar la carpeta del juego

## sorting
sort-a-z = Ordenado de A a Z. Pulsa para ordenar de Z a A
sort-z-a = Ordenado de Z a A. Pulsa para ordenar de A a Z

## store
store-add-to-library = Añadir a la biblioteca
store-title = Descubrir juegos
store-subtitle = Un catálogo creciente de juegos listos para añadir a tu biblioteca.
store-back = Volver a la tienda

## game
game-about = Sobre este juego
game-developer = Desarrollador
game-publisher = Distribuidor
game-tags = Etiquetas
game-play = Jugar
game-install = Instalar
game-pause = Pausar
game-resume = Reanudar
game-running = En ejecución
game-starting = Iniciando…
game-launch = Iniciar juego
game-action-tooltip = Iniciar, instalar o reanudar el juego
game-running-tooltip = El juego se está ejecutando
game-pause-download = Pausar descarga
game-resume-download = Reanudar descarga
game-pause-update = Pausar actualización
game-resume-update = Reanudar actualización
game-update = Actualizar juego
game-settings-tooltip = Ajustes del juego
game-error-download = No se pudo completar la descarga del juego
game-error-launch = No se pudo iniciar el juego
game-error-update = No se pudo actualizar el juego
game-error-runners = No se pudieron inicializar los runners
game-resuming = Reanudando…
game-preparing = Preparando…
game-preparing-update = Preparando actualización…

## progress
progress-paused = Pausado
progress-verifying = Verificando
progress-downloading = Descargando
progress-installing = Instalando
progress-validating = Validando
progress-finishing = Finalizando
progress-preparing = Preparando

## settings
settings-title = Ajustes del juego
settings-runtime = Runtime
settings-gamemode-title = Activar Feral GameMode
settings-gamemode-subtitle = Aplica temporalmente optimizaciones de rendimiento del sistema mientras el juego está en ejecución
settings-dxvk-hud-title = HUD de DXVK
settings-dxvk-hud-dx12-hint = Esta opción solo está disponible con DirectX 11. Para DirectX 12 puedes usar MangoHud como alternativa.
settings-dxvk-hud-subtitle = Elige qué información de rendimiento de DXVK se muestra en el juego
settings-dxvk-hud-none = Ninguno
settings-dxvk-hud-fps = FPS
settings-dxvk-hud-fps-frametime = FPS + Frametime
settings-dxvk-hud-fps-frametime-gpu = FPS + Frametime + GPU
settings-dxvk-hud-full = Completo
settings-graphics-api-title = API gráfica
settings-graphics-api-subtitle = DirectX 12 es la opción por defecto del juego; cambia a DirectX 11 si tienes problemas de renderizado o cierres inesperados
settings-graphics-api-dx12 = DirectX 12 (por defecto)
settings-graphics-api-dx11 = DirectX 11
settings-fsr-title = FSR (AMD FidelityFX Super Resolution)
settings-fsr-subtitle = Reescala el juego cuando se ejecuta a menor resolución que tu pantalla (requiere un runner con el fullscreen hack de Wine, como Proton-GE)
settings-fsr-quality-title = Calidad de FSR
settings-fsr-quality-subtitle = Preajuste que usa el reescalador
settings-fsr-ultra = Ultra calidad
settings-fsr-quality = Calidad
settings-fsr-balanced = Balanceado (por defecto)
settings-fsr-performance = Rendimiento
settings-sync-title = Sincronización de Wine
settings-sync-subtitle = NTSync se usa por defecto si tu sistema lo soporta; si no, FSync. La sincronización elegida y la que el juego usa realmente se guardan en el registro del juego
settings-sync-ntsync = NTSync
settings-sync-fsync = FSync
settings-sync-esync = ESync
settings-sync-none = Ninguno
settings-gamescope-title = Gamescope
settings-gamescope-subtitle = Ejecuta el juego dentro de gamescope
settings-gamescope-missing-native = No se encontró gamescope. Instálalo con el gestor de paquetes de tu distribución (por ejemplo: sudo dnf install gamescope)
settings-gamescope-missing-host = No se encontró gamescope en el host. Instálalo con el gestor de paquetes de tu distribución (por ejemplo: sudo dnf install gamescope)
settings-gamescope-missing-sandbox = No disponible dentro del sandbox de Flatpak. Instala la extensión (flatpak install flathub org.freedesktop.Platform.VulkanLayer.gamescope) o activa "Ejecutar en el host" para usar el gamescope del sistema
settings-host-title = Ejecutar en el sandbox de Flatpak
settings-host-subtitle = Solo Flatpak. Activado: el juego se ejecuta dentro del sandbox. Desactivado: se inicia en el sistema (equivale a BERRY_LAUNCH_ON_HOST=1), lo que además permite usar el gamescope del sistema
settings-host-forced = BERRY_LAUNCH_ON_HOST={ $value } está definido en el entorno: se ignorará el toggle y se respetará ese valor.
settings-gamescope-configure = Ajustes de gamescope
gamescope-title = Ajustes de gamescope
gamescope-display-group = Pantalla
gamescope-auto-hint = 0 = automático
gamescope-width = Ancho de salida
gamescope-height = Alto de salida
gamescope-game-width = Ancho del juego
gamescope-game-height = Alto del juego
gamescope-fps = Límite de FPS
gamescope-fps-unfocused = Límite de FPS sin foco
gamescope-mode-title = Modo de ventana
gamescope-mode-fullscreen = Pantalla completa
gamescope-mode-borderless = Sin bordes
gamescope-mode-windowed = En ventana
gamescope-scaling-group = Reescalado
gamescope-filter-title = Filtro de reescalado
gamescope-filter-default = Por defecto
gamescope-filter-fsr = FSR
gamescope-filter-nis = NIS
gamescope-filter-linear = Lineal
gamescope-filter-nearest = Vecino más cercano
gamescope-sharpness = Nitidez
gamescope-sharpness-subtitle = De 0 (más nítido) a 20 (más suave). Lo usan FSR y NIS
gamescope-integer = Escalado entero
gamescope-integer-subtitle = Escala solo por números enteros, manteniendo los píxeles nítidos
gamescope-misc-group = Otros
gamescope-hdr = HDR
gamescope-grab = Forzar captura del cursor
gamescope-extra = Argumentos adicionales
gamescope-extra-placeholder = p. ej. --mangoapp
settings-fps-title = Activar desbloqueador de FPS
settings-fps-subtitle = Desbloquea el límite de FPS del juego
settings-fps-target-title = FPS objetivo
settings-fps-target-subtitle = Aplicado por el desbloqueador de FPS al iniciar el juego
settings-fps-custom = Personalizado
settings-fps-custom-title = FPS personalizados
settings-fps-custom-subtitle = Tasa de fotogramas exacta que se aplica en lugar de un preset
settings-fps-refresh-title = Retardo de refresco (ms)
settings-fps-refresh-subtitle = Cada cuánto vuelve a aplicar el desbloqueador el valor de FPS mientras el juego está abierto
settings-fps-init-title = Retardo inicial (ms)
settings-fps-init-subtitle = Cuánto espera el desbloqueador tras iniciar el juego antes de buscar su ajuste de FPS
settings-compatibility = Compatibilidad
settings-runner = Runner
settings-remake-prefix-title = Rehacer prefijo
settings-remake-prefix-subtitle = Recrea el prefijo del runner seleccionado
settings-remake-prefix-button = Rehacer
settings-installation = Instalación
settings-location-title = Ubicación del juego
settings-location-choose = Elegir carpeta
settings-location-dialog = Elegir ubicación del juego
settings-repair-title = Reparar juego
settings-repair-subtitle = Verifica los archivos instalados y vuelve a descargar los datos dañados o ausentes
settings-repair-running = Reparando…
settings-repair-done = Reparación completada
settings-repair-done-detail = Los archivos del juego se han verificado y reparado correctamente.
settings-repair-failed = La reparación ha fallado
settings-env-title = Variables de entorno
settings-env-add = Añadir variable de entorno
settings-env-value = Valor
settings-env-variable = Variable
settings-env-remove = Eliminar variable

## status
status-fetching-manifest = Obteniendo el manifiesto del juego…
status-fetching-repair-manifest = Obteniendo el manifiesto de reparación…
status-installing = Instalando { $name } ({ $current }/{ $total })
status-repairing = Reparando { $name } ({ $current }/{ $total })

## runners
runners-title = Runners de compatibilidad
runners-subtitle = Instala y gestiona los runners Wine/Proton utilizados para iniciar tus juegos.
runners-show-all = Mostrar todos
runners-show-all-count = Mostrar todos ({ $count })
runners-cancel = Cancelar descarga

## catalog
catalog-genshin-impact =
    Bienvenidos a Teyvat, un continente de mil maravillas donde incontables criaturas prosperan en armonía.
    
    Regido por siete Arcontes, este mundo es un lugar donde los siete elementos convergen...

    Bajo cielos lejanos, un niño y una niña se encuentran en medio del tumulto.
    Son dos niños gemelos, y tú tomarás las riendas de la vida de uno de ellos. Una deidad aparece delante de ustedes, les separa y te hace caer en un profundo sueño.
    Cuando despiertas, el mundo ya no es el mismo que conocías...
    
    Genshin Impact es un juego de rol de mundo abierto. En él, descubrirás el fantástico mundo de Teyvat.
    
    Podrás visitar las siete diferentes naciones del continente y conocer a otros viajeros de todo el mundo para luchar juntos contra poderosos enemigos; además de descubrir los secretos que esconde y buscar a tu familiar perdido.
    
    ¡Explora libremente las diferentes regiones de Teyvat y deja que tu curiosidad te lleve a los rincones más místicos del continente!    

catalog-honkai-star-rail = 
    Honkai: Star Rail es el RPG de fantasía espacial de HoYoverse.
    Sube al Expreso Astral y descubre las maravillas infinitas de la galaxia en este viaje lleno de emoción y aventuras.
    Conocerás a nuevos compañeros en varios mundos e incluso encontrarás caras conocidas. Juntos tendrán que resolver los problemas causados por el Estelaron y descubrir los secretos que oculta. ¡Que esta travesía nos lleve hasta las estrellas!

    Descubre un universo lleno de sorpresas
    ¡El salto comienza en 3, 2, 1! Una Estación Espacial con objetos raros sellados, un planeta sumido en un invierno eterno, una flota gigante que caza abominaciones, un planeta de las celebraciones sumido en un dulce sueño, una tierra eterna con un destino incierto, hasta una arcadia que cayó en una pintura... ¡Cada parada del Expreso Astral es una mirada única a la galaxia! ¡Explora mundos y civilizaciones maravillosas y descubre misterios inimaginables en este viaje extraordinario!

catalog-zenless-zone-zero =
    «Zenless Zone Zero» es un nuevo juego de acción en 3D de HoYoverse que se desarrolla en un futuro próximo asolado por misteriosos fenómenos sobrenaturales conocidos como «Cavidades».

    Doble identidad, dos vidas distintas
    En un futuro próximo, tienen lugar misteriosos fenómenos sobrenaturales llamados «Cavidades». En medio de la catástrofe surge Nueva Eridu, una ciudad única por definición. Este último oasis domina la tecnología necesaria para garantizar la coexistencia con las Cavidades y alberga un conjunto de facciones caóticas, estrepitosas, peligrosas y muy dinámicas. Como proxy profesional, desempeñarás un papel importante conectando la ciudad con las Cavidades. Nueva Eridu será testigo de tu historia.

catalog-honkai-impact-3rd =
    La Buscadora de Sueños ha encontrado su primer sueño y la primera luz que se abre paso silenciosamente. "La forma que ahora adopto es el testimonio definitivo."

    Honkai Impact 3rd es un juego de acción y aventuras de ciencia ficción desarrollado por HoYoverse.

    Gráficos 3D cel-shading, combate 3D con salto libre, combos infinitos, controles precisos... ¡Experimenta la acción en tiempo real de última generación!

    Una historia original contada a través de diferentes medios, eventos inmersivos en el escenario y un elenco de voces estelar... ¡Forma parte de la leyenda!

    La crisis en la Tierra ha disminuido por ahora, mientras una nueva aventura se desarrolla en Marte.

    Conoce a Valkirias con personalidades únicas y explora junto a ellas los misterios de la civilización marciana.

catalog-wuthering-waves =
    Wuthering Waves es un juego de rol de mundo abierto lleno de historias y que permite un alto grado de libertad de juego. Despiertas como Errante, acompañado de un elenco vibrante de Resonadores, en un viaje para recuperar tus recuerdos perdidos, explorar y transformar el mundo.

    Te damos la bienvenida, Errante.
    La marea desciende dejando un silencio pesado.
    Golpeada por el Lamento, la humanidad sufre como si se hubiera encallado en un arrecife.
    Sin embargo, contraataca con un grito que rompe el silencio.
    La calamidad que trae la destrucción también envía el resurgimiento.
    Un nuevo mundo ha nacido.
    Errante, este es solo el comienzo de la historia, la tuya.
    Amigos que conocer, adversarios que derrotar, poderes nuevos que obtener, verdades secretas que descubrir y espectáculos nunca antes vistos que contemplar.
    Un mundo inmenso de infinitas posibilidades te aguarda. Pero la elección es tuya.
    Sé la respuesta, sé el líder y sigue los sonidos para llegar a un nuevo futuro.
    Partimos mientras la marea sube y baja.
    Levántate, Errante, esta es tu odisea.

## settings pages and groups
settings-page-general = General
settings-page-runner = Runner
settings-page-graphics = Gráficos
settings-page-performance = Rendimiento
settings-page-advanced = Avanzado
settings-group-performance = GameMode
settings-group-fps = Desbloqueo de FPS
settings-group-graphics = Gráficos
settings-group-upscaling = Escalado
settings-group-gamescope = Gamescope
settings-group-sync = Sincronización
settings-sync-detected = NTSync está disponible en este sistema y se usa por defecto.
settings-sync-not-detected = NTSync no está disponible en este sistema (requiere Linux 6.14+ con el módulo ntsync), por lo que se usa FSync por defecto.

## game settings
settings-launch-group = Lanzamiento
settings-launch-args-title = Argumentos de lanzamiento
settings-launch-args-subtitle = Argumentos extra de línea de comandos que se pasan al ejecutable del juego
settings-launch-args-placeholder = p. ej. -popupwindow
settings-wrapper-title = Prefijo de comando
settings-wrapper-subtitle = Programa que envuelve todo el lanzamiento, como mangohud o taskset -c 0-7
settings-wrapper-placeholder = p. ej. mangohud
settings-dedicated-gpu-title = Usar la GPU dedicada
settings-dedicated-gpu-subtitle = Pide a los portátiles con gráficos híbridos (NVIDIA Optimus, AMD) que rendericen el juego en la GPU dedicada
settings-maintenance-group = Mantenimiento
settings-open-button = Abrir
settings-open-game-folder-title = Carpeta del juego
settings-open-game-folder-subtitle = Abre la carpeta de instalación en tu gestor de archivos
settings-open-prefix-title = Carpeta del prefijo de Wine
settings-open-prefix-subtitle = Explora el prefijo del runner seleccionado
settings-prefix-missing = El prefijo aún no se ha creado. Se crea la primera vez que juegas
settings-open-logs-title = Registros del juego
settings-open-logs-subtitle = Abre la carpeta con el registro del último lanzamiento de cada juego
settings-shader-cache-title = Limpiar caché de shaders
settings-shader-cache-subtitle = Borra los shaders compilados de este juego. Se regeneran en el siguiente lanzamiento, lo que puede causar pequeños tirones
settings-shader-cache-button = Limpiar
settings-shader-cache-done = Caché de shaders limpiada
settings-shader-cache-done-detail = Se liberaron { $size }.
settings-shader-cache-failed = No se pudo limpiar la caché de shaders
settings-proton-log-title = Registro de depuración de Proton
settings-proton-log-subtitle = Escribe un registro detallado de Wine/Proton junto al prefijo. Útil para informar errores, pero ralentiza un poco el juego
settings-reset-title = Restablecer ajustes del juego
settings-reset-subtitle = Devuelve todos los ajustes de este juego a sus valores por defecto (se conservan los archivos del juego, la carpeta de instalación y el runner)
settings-reset-button = Restablecer
settings-reset-cancel = Cancelar
settings-reset-confirm = ¿Restablecer los ajustes de este juego?
settings-reset-confirm-detail = Todas las opciones, incluidas las variables de entorno personalizadas, vuelven a su valor por defecto.

## tags
tag-action = Acción
tag-adventure = Aventura
tag-rpg = RPG
tag-open-world = Mundo abierto
tag-turn-based = Por turnos
tag-multiplayer = Multijugador
tag-fantasy = Fantasía
tag-sci-fi = Ciencia ficción
tag-gacha = Gacha

## details window
details-title = Detalles de la operación
details-info-tooltip = Mostrar detalles de la operación
details-section-overview = Resumen
details-section-steps = Pasos
details-section-speed = Velocidad de red
details-section-log = Actividad
details-row-state = Estado
details-row-operation = Operación
details-row-version = Versión de destino
details-row-job = Trabajo actual
details-row-elapsed = Tiempo transcurrido
details-row-backend = Motor de descarga
details-state-running = En curso
details-state-completed = Completada
details-state-paused = Pausada
details-state-failed = Fallida
details-kind-install = Instalación
details-kind-update = Actualización
details-kind-repair = Reparación
details-step-resolving = Obtener información de descarga
details-step-verifying = Verificar archivos disponibles
details-step-downloading = Descargar archivos nuevos
details-step-extracting = Extraer archivos
details-step-finalizing = Finalizar
details-step-pending = Pendiente
details-step-skipped = No necesario
details-stat-current = Actual
details-stat-average = Media
details-stat-peak = Máxima
details-stat-remaining = Restante
details-stat-downloaded = Descargado
details-graph-waiting = Esperando datos de descarga…
details-job-none = Sin iniciar
details-log-started-install = Instalación iniciada
details-log-started-update = Actualización iniciada
details-log-started-repair = Reparación iniciada
details-log-plan = Versión { $version } mediante { $backend }, { $jobs } trabajo(s) de descarga
details-log-job = Iniciado { $name } ({ $current }/{ $total })
details-log-step = Paso: { $step }
details-log-completed = Finalizada correctamente
details-log-paused = Pausada por el usuario
details-log-failed = Error: { $error }
