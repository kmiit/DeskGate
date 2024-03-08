#include "pch.h"
#include "AdvancedPage.xaml.h"
#if __has_include("AdvancedPage.g.cpp")
#include "AdvancedPage.g.cpp"
#endif

using namespace winrt;
using namespace Microsoft::UI::Xaml;

// To learn more about WinUI, the WinUI project structure,
// and more about our project templates, see: http://aka.ms/winui-project-info.

namespace winrt::DeskGate::implementation
{
    int32_t AdvancedPage::MyProperty()
    {
        throw hresult_not_implemented();
    }

    void AdvancedPage::MyProperty(int32_t /* value */)
    {
        throw hresult_not_implemented();
    }

    void AdvancedPage::myButton_Click(IInspectable const&, RoutedEventArgs const&)
    {
        myButton().Content(box_value(L"Clicked"));
    }
}
