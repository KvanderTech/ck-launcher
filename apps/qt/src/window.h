#pragma once
#include "backend.h"
#include <QJsonArray>
#include <QMainWindow>
class QListWidget;
class QStackedWidget;
class QLabel;
class QPushButton;
class QTableWidget;
class QProgressBar;
class QLineEdit;
class QComboBox;
class QSpinBox;
class QPlainTextEdit;
class QCloseEvent;
class LauncherWindow final : public QMainWindow {
    Q_OBJECT
  public:
    explicit LauncherWindow(Backend *core, QWidget *parent = nullptr);
    void importPack(const QString &path = {});
    void initialize();

  protected:
    void closeEvent(QCloseEvent *event) override;

  private:
    Backend *core;
    QListWidget *navigation;
    QStackedWidget *pages;
    QLabel *status;
    QLabel *accountLabel;
    QLabel *libraryHint;
    QLabel *catalogStatus;
    QPushButton *play;
    QPushButton *stop;
    QProgressBar *progress;
    QTableWidget *buildsTable;
    QTableWidget *installedTable;
    QTableWidget *catalogTable;
    QTableWidget *accountsTable;
    QTableWidget *skinsTable;
    QLineEdit *searchText;
    QComboBox *searchType;
    QComboBox *versionFilter;
    QSpinBox *memory;
    QTableWidget *javaTable;
    QPlainTextEdit *logText;
    QComboBox *logFiles;
    QJsonArray builds, installed, catalog, accounts, skins, versions;
    QJsonObject profile;
    QString selectedBuild, selectedAccount, operationId;
    bool running = false;
    bool closing = false;
    bool busy = false;
    int catalogOffset = 0;
    quint64 catalogRequest = 0;
    void call(const QString &method, const QJsonObject &params = {},
              std::function<void(const QJsonValue &)> done = {}, bool mutation = false);
    void refreshLibrary();
    void refreshAccounts();
    void refreshRuntimes();
    void refreshContent();
    void loadVersions();
    void searchCatalog();
    void refreshSkins();
    void createBuild();
    void launch();
    void projectDetails();
    void browseFiles();
    void showLogs();
    void installCatalog();
    QJsonObject currentBuild() const;
    void message(const QString &text, bool error = false);
    QWidget *libraryPage();
    QWidget *catalogPage();
    QWidget *accountsPage();
    QWidget *settingsPage();
    QWidget *logsPage();
};
