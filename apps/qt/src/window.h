#pragma once
#include "backend.h"
#include "projectview.h"
#include "skinview.h"
#include "ui.h"
class LauncherWindow final : public QMainWindow {
    Q_OBJECT
  public:
    explicit LauncherWindow(Backend *core, QWidget *parent = nullptr);
    void importPack(const QString &path = {});
    void initialize();
    void showPage(const QString &name);
    void bringToFront();

  protected:
    void closeEvent(QCloseEvent *) override;
    void resizeEvent(QResizeEvent *) override;
#ifdef Q_OS_WIN
#if QT_VERSION >= QT_VERSION_CHECK(6, 0, 0)
    bool nativeEvent(const QByteArray &, void *, qintptr *) override;
#else
    bool nativeEvent(const QByteArray &, void *, long *) override;
#endif
#endif
  private:
    Backend *core;
    ImagePool *images;
    ProjectView *projectView = nullptr;
    int projectReturnPage = 2;
    Backdrop *background;
    QStackedWidget *pages, *skinPages, *detailSections;
    QVector<QPushButton *> navigation;
    QHBoxLayout *catalogBuilds;
    QVBoxLayout *sidebarBuilds, *catalogRows, *contentRows, *capeRows, *runtimeRows, *worldRows;
    CardGrid *libraryCards, *skinCards;
    QFrame *activity = nullptr;
    QLabel *status, *activityTitle, *libraryHint, *catalogStatus, *skinStatus, *previewName,
        *detailName, *gameDirectory, *updateStatus, *progressPercent, *progressDetails;
    QPushButton *accountButton, *play, *stop, *detailPlay, *addSkinButton;
    QProgressBar *progress;
    QLineEdit *searchText, *skinSearch, *filePath;
    QComboBox *versionFilter, *loaderFilter, *categoryFilter, *sortFilter, *buildFilter,
        *skinVariant, *logFiles;
    QCheckBox *hideInstalled;
    QTabBar *skinTabs;
    QSpinBox *memory;
    QSlider *memorySlider;
    QPlainTextEdit *logText;
    QTableWidget *filesTable;
    SkinView *skinPreview;
    Picture *detailIcon;
    QJsonArray builds, installed, catalog, accounts, skins, versions;
    QJsonObject profile, cosmetics;
    QString selectedBuild, selectedAccount, selectedSkin, operationId, activeMethod,
        catalogKind = QStringLiteral("modpack"), contentKind;
    bool running = false, closing = false, busy = false, ready = false, signingIn = false,
         cosmeticPending = false, contentInstalling = false;
    int currentPage = 0, catalogOffset = 0;
    quint64 catalogRequest = 0, skinRequest = 0;
    QSet<QString> completedOperations;
    qint64 cosmeticsLoadedAt = 0;
    void call(const QString &, const QJsonObject & = {},
              std::function<void(const QJsonValue &)> = {}, bool mutation = false);
    void navigate(int page);
    void message(const QString &, bool error = false);
    void showProgress(const QJsonObject &data);
    void refreshLibrary();
    void renderLibrary();
    void refreshContent();
    void renderContent();
    void refreshAccounts();
    void refreshSkins();
    void renderSkins();
    void renderCapes();
    void updateSkinPreview();
    void refreshRuntimes();
    void loadVersions();
    void searchCatalog();
    void renderCatalog();
    void createBuild();
    void openBuild(const QString &id);
    void buildSettings();
    void launch();
    void updatePlayState();
    void cancel();
    void accountMenu();
    void signIn();
    void addSkin();
    void skinAction(const QString &, const QJsonObject &);
    void projectDetails(const QJsonObject &project);
    void installCatalog(const QJsonObject &project);
    void browseFiles();
    void refreshFiles();
    void refreshWorlds();
    void showLogs();
    void addLocalContent();
    QJsonObject currentBuild() const;
    QJsonObject currentAccount() const;
    QWidget *homePage();
    QWidget *libraryPage();
    QWidget *detailPage();
    QWidget *catalogPage();
    QWidget *accountsPage();
    QWidget *settingsPage();
    QWidget *logsPage();
};
