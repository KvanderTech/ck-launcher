// Isolated visual QA harness. Never loads accounts, registers protocols, or
// starts the real service. Not installed or included in release archives.
#include "window.h"
#include <QTemporaryDir>
int main(int argc, char **argv) {
#if QT_VERSION < QT_VERSION_CHECK(6, 0, 0)
    QCoreApplication::setAttribute(Qt::AA_EnableHighDpiScaling);
    QCoreApplication::setAttribute(Qt::AA_UseHighDpiPixmaps);
#endif
    QApplication app(argc, argv);
    QTemporaryDir temporary;
    if (!temporary.isValid())
        return 1;
    app.setOrganizationName(s("CKLauncherTests"));
    app.setApplicationName(s("Preview"));
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, temporary.path());
    QSettings().setValue(s("sounds"), false);
    QSettings().setValue(s("motion"), false);
    app.setStyle(s("Fusion"));
    QFile style(s(":/assets/theme.qss"));
    if (style.open(QIODevice::ReadOnly))
        app.setStyleSheet(QString::fromUtf8(style.readAll()));
    Backend backend;
    LauncherWindow window(&backend);
    window.setWindowTitle(QString::fromUtf8("ЦК Лаунчер — изолированная проверка"));
    auto environment = QProcessEnvironment::systemEnvironment();
    environment.insert(s("APPDATA"), temporary.path());
    backend.start(app.applicationDirPath() + s("/ui-fixture.exe"), environment);
    QObject::connect(&app, &QCoreApplication::aboutToQuit, &backend, &Backend::shutdown);
    window.show();
    return app.exec();
}
