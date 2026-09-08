#pragma once
#include "ui.h"

class ProjectView final : public QWidget {
    Q_OBJECT
  public:
    ProjectView(Backend *backend, ImagePool *images, QWidget *parent = nullptr);
    void open(const QJsonObject &project, const QJsonArray &builds, const QString &selected);
    void setInstalling(bool active);
    void setInstallationError(const QString &message);
    void setActionBlockedReason(const QString &reason);
    static bool compatible(const QJsonObject &version, const QJsonObject &build,
                           const QString &type);
  signals:
    void back();
    void installRequested(const QString &project, const QString &version, const QString &build,
                          bool modpack);

  protected:
    void resizeEvent(QResizeEvent *event) override;

  private:
    Backend *backend;
    ImagePool *images;
    QJsonObject project;
    QJsonArray allVersions, builds;
    quint64 generation = 0;
    bool installing = false, projectLoading = false, versionsLoading = false, narrow = false;
    QString projectError, versionsError, installationError, actionBlockedReason;
    Picture *icon;
    QLabel *title, *description, *status, *versionCount, *emptyTitle, *emptyText;
    QTextBrowser *body;
    QComboBox *game, *loader, *version, *target;
    QPushButton *install, *retry;
    QTableWidget *versionTable;
    QTabBar *tabs;
    QStackedWidget *versionPages;
    QBoxLayout *columns;
    QGridLayout *installLayout;
    QWidget *leftColumn, *installPanel, *targetField, *gameField, *loaderField, *versionField;
    QLabel *installTitle;
    QScrollArea *installScroll;
    void filterVersions();
    void targetChanged();
    void selectVersion(const QString &id);
    void updateActions();
    void arrangeColumns();
    QJsonObject selectedBuild() const;
    void installSelected();
};
