#pragma once
#include "backend.h"
#include "widgets.h"
#include <QCache>
#include <QPointer>

QIcon glyph(const QString &name, const QColor &color = QColor(174, 199, 217), int size = 24);
QIcon roundedIcon(const QImage &image, int size = 36, qreal radius = 9);
QPushButton *iconButton(const QString &name, const QString &label, QWidget *parent = nullptr);
QFrame *panel(const QString &kind = QString());
QFrame *clickPanel(std::function<void()> action);
QLabel *label(const QString &text, const char *role = nullptr);
void clearLayout(QLayout *layout);
QWidget *scrollPage(QWidget *content);
void polish(QWidget *widget);

class AudioFeedback final : public QObject {
  public:
    explicit AudioFeedback(QObject *parent = nullptr);
    static void play(const QString &name);
    static bool isEnabled();
    static void setEnabled(bool enabled);

  protected:
    bool eventFilter(QObject *, QEvent *) override;
};

class Backdrop final : public QWidget {
  public:
    explicit Backdrop(QWidget *parent = nullptr);
    void setMotion(bool enabled);

  protected:
    void paintEvent(QPaintEvent *) override;

  private:
    QVector<QPixmap> scenes;
    QTimer cycle, fade;
    int current = 0, previous = 0;
    qreal blend = 1;
};

class CardGrid final : public QWidget {
  public:
    explicit CardGrid(int minimumWidth, int rowHeight, int maxColumns = 20,
                      QWidget *parent = nullptr);
    void append(QWidget *widget);
    void clear();
    void setCardWidth(int width);
    QSize minimumSizeHint() const override {
        return QSize(minimumWidth, 0);
    }
    int count() const {
        return items.size();
    }

  protected:
    void resizeEvent(QResizeEvent *) override;

  private:
    QGridLayout *grid;
    QVector<QWidget *> items;
    int minimumWidth, rowHeight, maxColumns, columns = 0, cardWidth = 0;
    void arrange();
};

class ImagePool final : public QObject {
  public:
    explicit ImagePool(Backend *backend, QObject *parent = nullptr);
    void load(const QString &url, QObject *owner, std::function<void(const QImage &)> done);
    static QImage decode(const QByteArray &bytes);
    static QUrl remoteUrl(const QString &url);

  private:
    struct Waiting {
        QPointer<QObject> owner;
        std::function<void(const QImage &)> done;
    };
    Backend *backend;
    QQueue<QString> queue;
    int activeRequests = 0;
    QCache<QString, QImage> cache;
    QHash<QString, QVector<Waiting>> waiting;
    void pump();
    void complete(const QString &key, const QImage &image);
};

class Picture final : public QWidget {
  public:
    explicit Picture(int size = 56, QWidget *parent = nullptr);
    void setImage(const QImage &image);
    void setFallback(const QString &text);
    bool pixelated = false;

  protected:
    void paintEvent(QPaintEvent *) override;

  private:
    QImage image;
    QString fallback;
};
